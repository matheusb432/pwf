use std::fmt::Write;

use pwf_client::v1::{
    BlockedByResolutionKind, BlockedByStatus, EffortTier, ListDetail, ListLayout, ListedTasks,
    TaskIssue, TaskIssueKind, TaskStatus, TaskStatusFilter, TaskView,
};

use super::{render_status, render_task_summary};

pub(in crate::task) fn render_list(result: &ListedTasks, location: &str, on: bool) -> String {
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
        render_grouped_list(&mut out, &result.tasks, status_filter, long, on);
    } else {
        let last_idx = result.tasks.len() - 1;
        for (idx, task) in result.tasks.iter().enumerate() {
            render_list_task(&mut out, task, status_filter, long, idx == last_idx, on);
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RenderGroup {
    Default,
    LowPrio,
    Human,
    Future,
    Other,
}

impl RenderGroup {
    fn from_section(section: Option<&str>) -> Self {
        match section {
            None => Self::Default,
            Some("Low-prio") => Self::LowPrio,
            Some("Human") => Self::Human,
            Some("Future") => Self::Future,
            Some(_) => Self::Other,
        }
    }

    fn title(self) -> Option<&'static str> {
        match self {
            Self::Default => None,
            Self::LowPrio => Some("Low-prio"),
            Self::Human => Some("Human"),
            Self::Future => Some("Future"),
            Self::Other => Some("Other"),
        }
    }
}

const RENDER_GROUPS: [RenderGroup; 5] = [
    RenderGroup::Default,
    RenderGroup::LowPrio,
    RenderGroup::Human,
    RenderGroup::Future,
    RenderGroup::Other,
];

fn render_grouped_list(
    out: &mut String,
    tasks: &[TaskView],
    status_filter: TaskStatusFilter,
    long: bool,
    on: bool,
) {
    let mut rendered_any = false;
    for group in RENDER_GROUPS {
        let group_tasks: Vec<&TaskView> = tasks
            .iter()
            .filter(|task| RenderGroup::from_section(task.section.as_deref()) == group)
            .collect();
        if group_tasks.is_empty() {
            continue;
        }
        if rendered_any {
            out.push_str("\n\n");
        }
        if let Some(title) = group.title() {
            out.push_str(title);
            out.push('\n');
        }
        for (idx, task) in group_tasks.iter().enumerate() {
            render_list_task(
                out,
                task,
                status_filter,
                long,
                idx + 1 == group_tasks.len(),
                on,
            );
        }
        rendered_any = true;
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
    task: &TaskView,
    status_filter: TaskStatusFilter,
    long: bool,
    last: bool,
    on: bool,
) {
    let task_status = TaskStatus::try_from(task.status).unwrap_or(TaskStatus::Unspecified);
    let status = if status_filter == TaskStatusFilter::All {
        Some(task_status)
    } else {
        None
    };
    let summary = render_task_summary(&task.id, &task.heading, status, on);
    let formatted = if last && !long {
        summary
    } else {
        format!("{summary}\n")
    };
    out.push_str(&formatted);
    if !long {
        return;
    }

    let _ = writeln!(out, "  status: {}", render_status(task_status, on));
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

    let _ = writeln!(out, "  project_path: {}", task.project_path);
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
    use pwf_client::v1::{BlockedByIssue, TaskLocation};

    use super::*;

    fn sample_task() -> TaskView {
        TaskView {
            id: "PWF-0064".to_string(),
            project: "pwf".to_string(),
            status: TaskStatus::Active as i32,
            heading: "make list commands formatting less redundant".to_string(),
            prompt: String::new(),
            project_path: "/project".to_string(),
            location: Some(TaskLocation {
                index_path: "pwf.md".to_string(),
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
        }
    }

    fn render_task_for_filter(
        task: &TaskView,
        status_filter: TaskStatusFilter,
        long: bool,
        on: bool,
    ) -> String {
        let mut output = String::new();
        render_list_task(&mut output, task, status_filter, long, true, on);
        output
    }

    #[test]
    fn all_status_short_lines_place_plain_lifecycle_after_the_identifier() {
        for (status, expected) in [
            (
                TaskStatus::Active,
                "PWF-0064 [active] :: make list commands formatting less redundant",
            ),
            (
                TaskStatus::Done,
                "PWF-0064 [done] :: make list commands formatting less redundant",
            ),
            (
                TaskStatus::Cancelled,
                "PWF-0064 [cancelled] :: make list commands formatting less redundant",
            ),
        ] {
            let mut task = sample_task();
            task.status = status as i32;
            let output = render_task_for_filter(&task, TaskStatusFilter::All, false, false);
            assert_eq!(output, expected);
            assert!(!output.contains('\u{1b}'), "{output}");
        }
    }

    #[test]
    fn exact_status_short_lines_keep_the_existing_shape() {
        let output = render_task_for_filter(&sample_task(), TaskStatusFilter::Active, false, false);
        assert_eq!(
            output,
            "PWF-0064 :: make list commands formatting less redundant"
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
            path: "/tasks/PWF-0064.md".to_string(),
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
        let result = ListedTasks {
            tasks: vec![sample_task()],
            hidden: 2,
            project: None,
            project_task_path: None,
            status_filter: TaskStatusFilter::Active as i32,
            layout: ListLayout::Flat as i32,
            detail: ListDetail::Summary as i32,
        };
        let output = render_list(&result, "notes", false);
        assert!(
            output.ends_with("\n... and 2 more; use '--all' to list everything"),
            "got: {output}"
        );
    }

    #[test]
    fn listed_detail_selects_metadata_rendering_without_a_second_flag() {
        let result = ListedTasks {
            tasks: vec![sample_task()],
            hidden: 0,
            project: None,
            project_task_path: None,
            status_filter: TaskStatusFilter::Active as i32,
            layout: ListLayout::Flat as i32,
            detail: ListDetail::Detailed as i32,
        };
        let output = render_list(&result, "notes", false);
        assert!(output.contains("  status: active\n"), "{output}");
        assert!(output.contains("  project_path: /project\n"), "{output}");
    }
}
