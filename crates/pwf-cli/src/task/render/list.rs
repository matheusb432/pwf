use std::fmt::Write;

use pwf_client::pb::{
    BlockedByResolutionKind, BlockedByStatus, EffortTier, ListDetail, ListLayout,
    ListTasksResponse, ListedTask, PriorityTier, TaskIssue, TaskIssueKind, TaskStatus,
    TaskStatusFilter,
};
use pwf_models::settings::TaskStatusColors;

use super::{render_status, render_task_summary};

pub(in crate::task) fn render_list(
    result: &ListTasksResponse,
    location: &str,
    task_status_colors: TaskStatusColors,
    on: bool,
) -> String {
    let detail = ListDetail::try_from(result.detail).unwrap_or(ListDetail::Unspecified);
    let status_filter =
        TaskStatusFilter::try_from(result.status_filter).unwrap_or(TaskStatusFilter::Unspecified);
    let long = detail == ListDetail::Detailed;
    if result.tasks.is_empty() {
        return match status_filter {
            TaskStatusFilter::Active => {
                format!("No active task prompts found in {location}.\n")
            }
            TaskStatusFilter::Done => {
                format!("No task prompts with status done found in {location}.\n")
            }
            TaskStatusFilter::Cancelled => {
                format!("No task prompts with status cancelled found in {location}.\n")
            }
            TaskStatusFilter::All | TaskStatusFilter::Unspecified => {
                format!("No task prompts found in {location}.\n")
            }
        };
    }

    let mut out = String::new();
    let layout = ListLayout::try_from(result.layout).unwrap_or(ListLayout::Unspecified);
    if layout == ListLayout::BySection {
        render_grouped_list(
            &mut out,
            &result.tasks,
            status_filter,
            long,
            task_status_colors,
            on,
        );
    } else {
        let last_idx = result.tasks.len() - 1;
        for (idx, task) in result.tasks.iter().enumerate() {
            render_list_task(
                &mut out,
                task,
                status_filter,
                long,
                idx == last_idx,
                task_status_colors,
                on,
            );
        }
    }
    if result.hidden > 0 {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        let _ = write!(
            out,
            "... and {} more; use '--all' to list everything",
            result.hidden
        );
    }
    out
}

fn render_grouped_list(
    out: &mut String,
    tasks: &[ListedTask],
    status_filter: TaskStatusFilter,
    long: bool,
    task_status_colors: TaskStatusColors,
    on: bool,
) {
    let mut group_start = 0;
    while group_start < tasks.len() {
        let section = tasks[group_start].section.as_deref();
        let section_key = section.map(str::to_lowercase);
        let group_end = tasks[group_start + 1..]
            .iter()
            .position(|task| task.section.as_deref().map(str::to_lowercase) != section_key)
            .map_or(tasks.len(), |offset| group_start + offset + 1);

        if group_start > 0 {
            out.push_str("\n\n");
        }
        if let Some(title) = section {
            out.push_str(title);
            out.push('\n');
        }
        let group = &tasks[group_start..group_end];
        for (index, task) in group.iter().enumerate() {
            render_list_task(
                out,
                task,
                status_filter,
                long,
                index + 1 == group.len(),
                task_status_colors,
                on,
            );
        }
        group_start = group_end;
    }
}

fn blocked_by_status_summary(statuses: &[BlockedByStatus]) -> String {
    statuses
        .iter()
        .map(|blocked_by| {
            let status = match BlockedByResolutionKind::try_from(blocked_by.resolution).ok() {
                Some(BlockedByResolutionKind::Found) => blocked_by
                    .status
                    .and_then(|status| TaskStatus::try_from(status).ok())
                    .map_or("unspecified", task_status_name)
                    .to_string(),
                Some(BlockedByResolutionKind::Missing) => "missing".to_string(),
                Some(BlockedByResolutionKind::Unavailable) => format!(
                    "unavailable: {}",
                    blocked_by
                        .reason
                        .as_deref()
                        .unwrap_or("unknown")
                        .replace(['\r', '\n'], " ")
                ),
                Some(BlockedByResolutionKind::Unspecified) | None => {
                    "unavailable: invalid status".to_string()
                }
            };
            format!("{} ({status})", blocked_by.id)
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_list_task(
    out: &mut String,
    task: &ListedTask,
    status_filter: TaskStatusFilter,
    long: bool,
    last: bool,
    task_status_colors: TaskStatusColors,
    on: bool,
) {
    let task_status = TaskStatus::try_from(task.status).unwrap_or(TaskStatus::Unspecified);
    let summary = render_task_summary(
        &task.id,
        &task.heading,
        task_status,
        status_filter == TaskStatusFilter::All && !on,
        task_status_colors,
        on,
    );
    let formatted = if last && !long {
        summary
    } else {
        format!("{summary}\n")
    };
    out.push_str(&formatted);
    if !long {
        return;
    }

    let _ = writeln!(
        out,
        "  status: {}",
        render_status(task_status, task_status_colors, on)
    );
    if task_status == TaskStatus::Active {
        let relationship_warning = !task.blocked_by_issues.is_empty()
            || task.blocked_by_statuses.iter().any(blocked_by_is_warning);
        let launch_ready = task.launch_issues.is_empty();
        let launch = if launch_ready && relationship_warning {
            "READY WITH WARNINGS"
        } else if launch_ready {
            "READY"
        } else {
            "NEEDS ATTENTION"
        };
        let _ = writeln!(out, "  launch: {launch}");
        if task.launch_issues.iter().any(|issue| {
            TaskIssueKind::try_from(issue.kind).ok() == Some(TaskIssueKind::PlaceholderPrompt)
        }) {
            out.push_str("  launch: NEEDS PROMPT\n");
        }
    }

    let _ = writeln!(
        out,
        "  project_path: {}",
        task.project_path.as_deref().unwrap_or("none")
    );
    if let Some(location) = &task.location {
        let _ = writeln!(out, "  note: {}:{}", location.index_path, location.line);
    } else {
        out.push_str("  note: (unavailable)\n");
    }
    let prompt = task.prompt.replace("\r\n", " / ").replace('\n', " / ");
    let _ = writeln!(out, "  prompt: {prompt}");
    if !task.blocked_by_statuses.is_empty() {
        let _ = writeln!(
            out,
            "  blocked_by: {}",
            blocked_by_status_summary(&task.blocked_by_statuses)
        );
    }
    if let Some(effort) = task
        .effort
        .and_then(|effort| EffortTier::try_from(effort).ok())
    {
        let _ = writeln!(out, "  effort: {}", effort_name(effort));
    }
    if let Some(priority) = task
        .priority
        .and_then(|priority| PriorityTier::try_from(priority).ok())
    {
        let _ = writeln!(out, "  priority: {}", priority_name(priority));
    }
    if let Some(tags) = &task.raw_tags {
        let _ = writeln!(out, "  tags: {tags}");
    }
    for issue in &task.blocked_by_issues {
        let _ = writeln!(
            out,
            "  issue: Malformed blocked_by metadata {:?} in {}: {}",
            issue.raw, issue.path, issue.reason
        );
    }
    if task_status == TaskStatus::Active {
        for issue in &task.launch_issues {
            let _ = writeln!(out, "  issue: {}", launch_issue(issue));
        }
        if !task.launch_issues.is_empty() {
            let index_path = task
                .location
                .as_ref()
                .map_or("the task index", |location| location.index_path.as_str());
            let _ = writeln!(
                out,
                "  fix: edit {index_path} or update the managed project record"
            );
        }
    }
}

fn blocked_by_is_warning(blocked_by: &BlockedByStatus) -> bool {
    BlockedByResolutionKind::try_from(blocked_by.resolution).ok()
        != Some(BlockedByResolutionKind::Found)
        || blocked_by
            .status
            .and_then(|status| TaskStatus::try_from(status).ok())
            != Some(TaskStatus::Done)
}

fn launch_issue(issue: &TaskIssue) -> String {
    match TaskIssueKind::try_from(issue.kind).ok() {
        Some(TaskIssueKind::MissingNote) => format!(
            "Task note missing: {}",
            issue.path.as_deref().unwrap_or("(unknown)")
        ),
        Some(TaskIssueKind::PlaceholderPrompt) => {
            "Prompt is a placeholder; define a real prompt before launching.".to_string()
        }
        Some(TaskIssueKind::Unspecified) | None => "Invalid launch issue from server.".to_string(),
    }
}

fn effort_name(effort: EffortTier) -> &'static str {
    match effort {
        EffortTier::Low => "low",
        EffortTier::Medium => "medium",
        EffortTier::High => "high",
        EffortTier::Highest => "highest",
        EffortTier::Unspecified => "unspecified",
    }
}

fn priority_name(priority: PriorityTier) -> &'static str {
    match priority {
        PriorityTier::Low => "low",
        PriorityTier::Medium => "medium",
        PriorityTier::High => "high",
        PriorityTier::Highest => "highest",
        PriorityTier::Unspecified => "unspecified",
    }
}

fn task_status_name(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Active => "active",
        TaskStatus::Done => "done",
        TaskStatus::Cancelled => "cancelled",
        TaskStatus::Unspecified => "unspecified",
    }
}

#[cfg(test)]
mod tests {
    use pwf_client::pb::{BlockedByIssue, TaskLocation};
    use pwf_models::settings::RgbColor;

    use super::*;
    use crate::test_style::{assert_plain, color_rgb};

    fn sample_task() -> ListedTask {
        ListedTask {
            id: "FOO-0001".to_string(),
            project: "foo".to_string(),
            status: TaskStatus::Active as i32,
            heading: "sample task".to_string(),
            prompt: String::new(),
            project_path: Some("/project".to_string()),
            location: Some(TaskLocation {
                index_path: "foo.md".to_string(),
                line: 1,
            }),
            launch_issues: Vec::new(),
            section: None,
            blocked_by: Vec::new(),
            blocked_by_statuses: Vec::new(),
            blocked_by_issues: Vec::new(),
            effort: None,
            raw_tags: None,
            created: None,
            priority: None,
        }
    }

    fn render_task_for_filter(
        task: &ListedTask,
        status_filter: TaskStatusFilter,
        long: bool,
        on: bool,
    ) -> String {
        let mut output = String::new();
        render_list_task(
            &mut output,
            task,
            status_filter,
            long,
            true,
            TaskStatusColors::default(),
            on,
        );
        output
    }

    #[test]
    fn all_status_short_lines_place_plain_lifecycle_after_the_identifier() {
        for (status, expected) in [
            (TaskStatus::Active, "FOO-0001 [active] :: sample task"),
            (TaskStatus::Done, "FOO-0001 [done] :: sample task"),
            (TaskStatus::Cancelled, "FOO-0001 [cancelled] :: sample task"),
        ] {
            let mut task = sample_task();
            task.status = status as i32;
            let output = render_task_for_filter(&task, TaskStatusFilter::All, false, false);
            assert_eq!(output, expected);
            assert_plain(&output);
        }
    }

    #[test]
    fn all_status_short_lines_color_identifiers_by_lifecycle() {
        for (status, color) in [
            (TaskStatus::Active, color_rgb(100, 149, 237)),
            (TaskStatus::Done, color_rgb(163, 230, 53)),
            (TaskStatus::Cancelled, color_rgb(255, 107, 138)),
        ] {
            let mut task = sample_task();
            task.status = status as i32;
            let output = render_task_for_filter(&task, TaskStatusFilter::All, false, true);
            assert!(
                output.contains(&format!("{color}FOO-0001{color:#}")),
                "{output:?}"
            );
        }
    }

    #[test]
    fn exact_active_status_short_line_uses_the_default_active_color() {
        let output = render_task_for_filter(&sample_task(), TaskStatusFilter::Active, false, true);

        assert_eq!(
            output,
            format!(
                "{blue}FOO-0001{blue:#} :: sample task",
                blue = color_rgb(100, 149, 237)
            )
        );
    }

    #[test]
    fn configured_lifecycle_colors_apply_to_identifiers_without_status_tags() {
        let colors = TaskStatusColors::new(
            Some(RgbColor::new(1, 2, 3)),
            Some(RgbColor::new(4, 5, 6)),
            Some(RgbColor::new(7, 8, 9)),
        );
        for (status, color) in [
            (TaskStatus::Active, RgbColor::new(1, 2, 3)),
            (TaskStatus::Done, RgbColor::new(4, 5, 6)),
            (TaskStatus::Cancelled, RgbColor::new(7, 8, 9)),
        ] {
            let mut task = sample_task();
            task.status = status as i32;
            let mut output = String::new();
            render_list_task(
                &mut output,
                &task,
                TaskStatusFilter::All,
                false,
                true,
                colors,
                true,
            );
            let style = color_rgb(color.red(), color.green(), color.blue());
            assert!(output.contains(&format!("{style}FOO-0001{style:#}")));
            assert!(!output.contains(task_status_name(status)));
        }
    }

    #[test]
    fn exact_status_short_lines_keep_the_existing_shape() {
        let output = render_task_for_filter(&sample_task(), TaskStatusFilter::Active, false, false);
        assert_eq!(output, "FOO-0001 :: sample task");
    }

    #[test]
    fn grouped_list_renders_real_section_names_and_merges_case_insensitively() {
        let mut unsectioned = sample_task();
        unsectioned.id = "FOO-0001".to_string();
        let mut blocked = sample_task();
        blocked.id = "FOO-0002".to_string();
        blocked.section = Some("Blocked".to_string());
        let mut blocked_case_variant = sample_task();
        blocked_case_variant.id = "AUX-0001".to_string();
        blocked_case_variant.section = Some("blocked".to_string());
        let mut someday = sample_task();
        someday.id = "FOO-0003".to_string();
        someday.section = Some("Someday".to_string());
        let result = ListTasksResponse {
            tasks: vec![unsectioned, blocked, blocked_case_variant, someday],
            hidden: 0,
            project: None,
            project_task_path: None,
            status_filter: TaskStatusFilter::Active as i32,
            layout: ListLayout::BySection as i32,
            detail: ListDetail::Summary as i32,
            next_page_token: None,
        };

        assert_eq!(
            render_list(&result, "notes", TaskStatusColors::default(), false),
            "FOO-0001 :: sample task\n\nBlocked\nFOO-0002 :: sample task\nAUX-0001 :: sample task\n\nSomeday\nFOO-0003 :: sample task"
        );
    }

    #[test]
    fn long_form_separates_lifecycle_from_active_launch_readiness() {
        let output = render_task_for_filter(&sample_task(), TaskStatusFilter::Active, true, false);
        assert!(output.contains("  status: active\n"), "{output}");
        assert!(output.contains("  launch: READY\n"), "{output}");
        assert!(output.contains("  project_path: /project\n"), "{output}");
    }

    #[test]
    fn closed_long_form_omits_active_launch_diagnostics() {
        let mut task = sample_task();
        task.status = TaskStatus::Done as i32;
        task.launch_issues = vec![TaskIssue {
            kind: TaskIssueKind::PlaceholderPrompt as i32,
            path: None,
        }];
        let output = render_task_for_filter(&task, TaskStatusFilter::Done, true, false);
        assert!(output.contains("  status: done\n"), "{output}");
        assert!(!output.contains("launch:"), "{output}");
        assert!(!output.contains("issue:"), "{output}");
        assert!(!output.contains("fix:"), "{output}");
    }

    #[test]
    fn long_form_formats_typed_blocked_by_statuses() {
        let mut task = sample_task();
        task.blocked_by_statuses = vec![
            BlockedByStatus {
                id: "AUX-0014".to_string(),
                title: None,
                resolution: BlockedByResolutionKind::Found as i32,
                status: Some(TaskStatus::Done as i32),
                reason: None,
            },
            BlockedByStatus {
                id: "AUX-0015".to_string(),
                title: None,
                resolution: BlockedByResolutionKind::Found as i32,
                status: Some(TaskStatus::Active as i32),
                reason: None,
            },
            BlockedByStatus {
                id: "AUX-9999".to_string(),
                title: None,
                resolution: BlockedByResolutionKind::Missing as i32,
                status: None,
                reason: None,
            },
            BlockedByStatus {
                id: "ALT-0001".to_string(),
                title: None,
                resolution: BlockedByResolutionKind::Unavailable as i32,
                status: None,
                reason: Some("vault read failed".to_string()),
            },
        ];
        let output = render_task_for_filter(&task, TaskStatusFilter::Active, true, false);
        assert!(
            output.contains(
                "  blocked_by: AUX-0014 (done), AUX-0015 (active), AUX-9999 (missing), ALT-0001 (unavailable: vault read failed)\n"
            ),
            "{output}"
        );
        assert!(
            output.contains("  launch: READY WITH WARNINGS\n"),
            "{output}"
        );
    }

    #[test]
    fn long_form_reports_malformed_blocked_by_without_making_the_task_unlaunchable() {
        let mut task = sample_task();
        task.blocked_by_issues = vec![BlockedByIssue {
            path: "/tasks/FOO-0064.md".to_string(),
            raw: "\"[[AUX-0001]]\"".to_string(),
            reason: "expected a sequence".to_string(),
        }];
        let output = render_task_for_filter(&task, TaskStatusFilter::Active, true, false);
        assert!(
            output.contains("  launch: READY WITH WARNINGS\n"),
            "{output}"
        );
        assert!(
            output.contains("  issue: Malformed blocked_by metadata"),
            "{output}"
        );
    }

    #[test]
    fn list_footer_mentions_hidden_count_and_escape_hatch() {
        let result = ListTasksResponse {
            tasks: vec![sample_task()],
            hidden: 2,
            project: None,
            project_task_path: None,
            status_filter: TaskStatusFilter::Active as i32,
            layout: ListLayout::Flat as i32,
            detail: ListDetail::Summary as i32,
            next_page_token: None,
        };
        let output = render_list(&result, "notes", TaskStatusColors::default(), false);
        assert!(
            output.ends_with("\n... and 2 more; use '--all' to list everything"),
            "got: {output}"
        );
    }

    #[test]
    fn listed_detail_selects_metadata_rendering_without_a_second_flag() {
        let result = ListTasksResponse {
            tasks: vec![sample_task()],
            hidden: 0,
            project: None,
            project_task_path: None,
            status_filter: TaskStatusFilter::Active as i32,
            layout: ListLayout::Flat as i32,
            detail: ListDetail::Detailed as i32,
            next_page_token: None,
        };
        let output = render_list(&result, "notes", TaskStatusColors::default(), false);
        assert!(output.contains("  status: active\n"), "{output}");
        assert!(output.contains("  project_path: /project\n"), "{output}");
    }
}
