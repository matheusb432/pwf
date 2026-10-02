use std::fmt::Write;

use pwf_client::pb::{
    BlockedByResolutionKind, BlockedByStatus, ListTasksResponse, ListedTask, TaskIssue,
    TaskIssueKind, TaskStatus, TaskStatusFilter,
};
use pwf_models::settings::UserSettings;

use super::render_task_summary;

pub(in crate::task) fn render_list(
    result: &ListTasksResponse,
    location: &str,
    settings: &UserSettings,
    on: bool,
) -> String {
    let status_filter =
        TaskStatusFilter::try_from(result.status_filter).unwrap_or(TaskStatusFilter::Unspecified);
    if result.tasks.is_empty() {
        return match status_filter {
            TaskStatusFilter::Active => {
                format!("No active task bodies found in {location}.\n")
            }
            TaskStatusFilter::Backlog => {
                format!("No task bodies with status backlog found in {location}.\n")
            }
            TaskStatusFilter::Done => {
                format!("No task bodies with status done found in {location}.\n")
            }
            TaskStatusFilter::Cancelled => {
                format!("No task bodies with status cancelled found in {location}.\n")
            }
            TaskStatusFilter::All | TaskStatusFilter::Unspecified => {
                format!("No task bodies found in {location}.\n")
            }
        };
    }

    let mut out = String::new();
    let last_idx = result.tasks.len() - 1;
    for (idx, task) in result.tasks.iter().enumerate() {
        render_list_task(&mut out, task, status_filter, idx == last_idx, settings, on);
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

pub(super) fn blocked_by_status_summary(statuses: &[BlockedByStatus]) -> String {
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
    last: bool,
    settings: &UserSettings,
    on: bool,
) {
    let status = TaskStatus::try_from(task.status).unwrap_or(TaskStatus::Unspecified);
    out.push_str(&render_task_summary(
        &task.id,
        &task.heading,
        status,
        status_filter == TaskStatusFilter::All && !on,
        settings,
        on,
    ));
    if !last {
        out.push('\n');
    }
}

pub(super) fn diagnostics(task: &ListedTask) -> String {
    let mut output = String::new();
    for issue in &task.blocked_by_issues {
        let _ = writeln!(
            output,
            "Issue: Malformed blocked_by metadata {:?} in {}: {}",
            issue.raw, issue.path, issue.reason
        );
    }
    if TaskStatus::try_from(task.status).ok() == Some(TaskStatus::Active) {
        if !task.launch_issues.is_empty() {
            for issue in &task.launch_issues {
                let _ = writeln!(output, "Issue: {}", launch_issue(*issue));
            }
            let _ = writeln!(output, "Fix: edit {}", task.file_path);
        } else if !task.blocked_by_issues.is_empty()
            || task.blocked_by_statuses.iter().any(blocked_by_is_warning)
        {
            output.push_str("Launch: READY WITH WARNINGS\n");
        }
    }
    output
}

fn blocked_by_is_warning(blocked_by: &BlockedByStatus) -> bool {
    BlockedByResolutionKind::try_from(blocked_by.resolution).ok()
        != Some(BlockedByResolutionKind::Found)
        || blocked_by
            .status
            .and_then(|status| TaskStatus::try_from(status).ok())
            != Some(TaskStatus::Done)
}

fn launch_issue(issue: TaskIssue) -> &'static str {
    match TaskIssueKind::try_from(issue.kind).ok() {
        Some(TaskIssueKind::PlaceholderBody) => {
            "Body is a placeholder; define a real body before launching."
        }
        Some(TaskIssueKind::Unspecified) | None => "Invalid launch issue from server.",
    }
}

fn task_status_name(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Active => "active",
        TaskStatus::Done => "done",
        TaskStatus::Backlog => "backlog",
        TaskStatus::Cancelled => "cancelled",
        TaskStatus::Unspecified => "unspecified",
    }
}

#[cfg(test)]
mod tests {
    use pwf_models::settings::{NoteStatusColors, ProjectStatusColors, RgbColor, TaskStatusColors};

    use super::*;
    use crate::test_style::{assert_plain, color_rgb};

    fn sample_task() -> ListedTask {
        ListedTask {
            id: "FOO-0001".to_string(),
            project: "foo".to_string(),
            status: TaskStatus::Active as i32,
            heading: "sample task".to_string(),
            body: String::new(),
            project_path: Some("/project".to_string()),
            file_path: "FOO-0001.md".to_string(),
            launch_issues: Vec::new(),

            blocked_by: Vec::new(),
            blocked_by_statuses: Vec::new(),
            blocked_by_issues: Vec::new(),
            effort: None,
            raw_tags: None,
            created: None,
            priority: None,
            ..Default::default()
        }
    }

    fn render_task_for_filter(
        task: &ListedTask,
        status_filter: TaskStatusFilter,
        on: bool,
    ) -> String {
        let mut output = String::new();
        render_list_task(
            &mut output,
            task,
            status_filter,
            true,
            &UserSettings::default(),
            on,
        );
        output
    }

    #[test]
    fn all_status_short_lines_place_plain_lifecycle_after_the_identifier() {
        for (status, expected) in [
            (TaskStatus::Active, "FOO-0001 [active] :: sample task"),
            (TaskStatus::Done, "FOO-0001 [done] :: sample task"),
            (TaskStatus::Backlog, "FOO-0001 [backlog] :: sample task"),
            (TaskStatus::Cancelled, "FOO-0001 [cancelled] :: sample task"),
        ] {
            let mut task = sample_task();
            task.status = status as i32;
            let output = render_task_for_filter(&task, TaskStatusFilter::All, false);
            assert_eq!(output, expected);
            assert_plain(&output);
        }
    }

    #[test]
    fn all_status_short_lines_color_identifiers_by_lifecycle() {
        for (status, color) in [
            (TaskStatus::Active, color_rgb(100, 149, 237)),
            (TaskStatus::Done, color_rgb(163, 230, 53)),
            (TaskStatus::Backlog, color_rgb(234, 179, 8)),
            (TaskStatus::Cancelled, color_rgb(255, 107, 138)),
        ] {
            let mut task = sample_task();
            task.status = status as i32;
            let output = render_task_for_filter(&task, TaskStatusFilter::All, true);
            assert!(
                output.contains(&format!("{color}FOO-0001{color:#}")),
                "{output:?}"
            );
        }
    }

    #[test]
    fn exact_active_status_short_line_uses_the_default_active_color() {
        let output = render_task_for_filter(&sample_task(), TaskStatusFilter::Active, true);

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
            Some(RgbColor::new(10, 11, 12)),
        );
        let settings = UserSettings::new(
            colors,
            ProjectStatusColors::default(),
            NoteStatusColors::default(),
            pwf_models::task::PriorityTier::Medium,
            pwf_models::task::order::OrderSpec::default(),
        );
        for (status, color) in [
            (TaskStatus::Active, RgbColor::new(1, 2, 3)),
            (TaskStatus::Done, RgbColor::new(4, 5, 6)),
            (TaskStatus::Cancelled, RgbColor::new(7, 8, 9)),
            (TaskStatus::Backlog, RgbColor::new(10, 11, 12)),
        ] {
            let mut task = sample_task();
            task.status = status as i32;
            let mut output = String::new();
            render_list_task(
                &mut output,
                &task,
                TaskStatusFilter::All,
                true,
                &settings,
                true,
            );
            let style = color_rgb(color.red(), color.green(), color.blue());
            assert!(output.contains(&format!("{style}FOO-0001{style:#}")));
            assert!(!output.contains(task_status_name(status)));
        }
    }

    #[test]
    fn exact_status_short_lines_keep_the_existing_shape() {
        let output = render_task_for_filter(&sample_task(), TaskStatusFilter::Active, false);
        assert_eq!(output, "FOO-0001 :: sample task");
    }
    #[test]
    fn rich_diagnostics_keep_lineage_and_report_malformed_relationships() {
        let mut task = sample_task();
        task.blocked_by_statuses = vec![BlockedByStatus {
            id: "AUX-0014".to_string(),
            resolution: BlockedByResolutionKind::Found as i32,
            status: Some(TaskStatus::Done as i32),
            ..Default::default()
        }];
        assert_eq!(
            blocked_by_status_summary(&task.blocked_by_statuses),
            "AUX-0014 (done)"
        );
        assert_eq!(diagnostics(&task), "");
        task.blocked_by_statuses[0].status = Some(TaskStatus::Active as i32);
        assert!(diagnostics(&task).contains("READY WITH WARNINGS"));
        task.blocked_by_issues.push(pwf_client::pb::BlockedByIssue {
            raw: "bad links".to_string(),
            path: task.file_path.clone(),
            reason: "expected wikilinks".to_string(),
        });
        assert!(diagnostics(&task).contains("Malformed blocked_by metadata"));
    }

    #[test]
    fn launch_diagnostics_apply_only_to_active_tasks() {
        let mut task = sample_task();
        task.launch_issues.push(TaskIssue {
            kind: TaskIssueKind::PlaceholderBody as i32,
        });
        let output = diagnostics(&task);
        assert!(output.contains("Body is a placeholder"));
        assert!(output.contains("Fix: edit FOO-0001.md"));
        task.status = TaskStatus::Done as i32;
        assert_eq!(diagnostics(&task), "");
    }
}
