use std::fmt::Write;

use pwf_application::task::{ListTasksOk, PrerequisiteStatus, StatusFilter, TaskView};
use pwf_models::task::TaskStatus;

use super::{StatusPlacement, render_status, render_task_summary};

pub(in crate::task) fn render_list(
    result: &ListTasksOk,
    location: &str,
    status_filter: StatusFilter,
    long: bool,
    grouped: bool,
    on: bool,
) -> String {
    if result.tasks.is_empty() {
        return match status_filter {
            StatusFilter::Exact(TaskStatus::Active) => {
                format!("No active task prompts found in {location}.\n")
            }
            StatusFilter::Exact(status) => {
                format!("No task prompts with status {status} found in {location}.\n")
            }
            StatusFilter::All => {
                format!("No task prompts found in {location}.\n")
            }
        };
    }
    let mut out = String::new();
    if grouped {
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
    status_filter: StatusFilter,
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

fn prerequisite_status_summary(statuses: &[PrerequisiteStatus]) -> String {
    statuses
        .iter()
        .map(|prerequisite| {
            let status = prerequisite
                .status
                .map_or_else(|| "missing".to_string(), |status| status.to_string());
            format!("{} ({status})", prerequisite.id)
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_list_task(
    out: &mut String,
    task: &TaskView,
    status_filter: StatusFilter,
    long: bool,
    last: bool,
    on: bool,
) {
    let status = if status_filter == StatusFilter::All {
        Some((task.status, StatusPlacement::AfterIdentifier))
    } else {
        None
    };
    let summary = render_task_summary(&task.id, &task.session, status, on);
    let formatted = if last && !long {
        summary
    } else {
        format!("{summary}\n")
    };
    out.push_str(&formatted);
    if !long {
        return;
    }
    let _ = writeln!(out, "  status: {}", render_status(task.status, on));
    if task.status == TaskStatus::Active {
        let launch = if task.launchable {
            "READY"
        } else {
            "NEEDS ATTENTION"
        };
        let _ = writeln!(out, "  launch: {launch}");
        if task.needs_prompt {
            out.push_str("  launch: NEEDS PROMPT\n");
        }
    }
    match &task.repo {
        Some(r) if !r.is_empty() => {
            let _ = writeln!(out, "  repo: {r}");
        }
        _ => out.push_str("  repo: (not configured)\n"),
    }
    let _ = writeln!(out, "  note: {}:{}", task.note, task.line);
    let prompt = task.prompt.replace("\r\n", " / ").replace('\n', " / ");
    let _ = writeln!(out, "  prompt: {prompt}");
    if !task.prerequisite_statuses.is_empty() {
        let _ = writeln!(
            out,
            "  prereq: {}",
            prerequisite_status_summary(&task.prerequisite_statuses)
        );
    }
    if let Some(e) = &task.effort {
        let _ = writeln!(out, "  effort: {e}");
    }
    if let Some(tags) = &task.tags {
        let _ = writeln!(out, "  tags: {tags}");
    }
    if task.status == TaskStatus::Active {
        for issue in &task.issues {
            let _ = writeln!(out, "  issue: {issue}");
        }
        if !task.launchable {
            let _ = writeln!(
                out,
                "  fix: edit {} or update the managed project record",
                task.note
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use pwf_application::task::{PrerequisiteStatus, StatusFilter};
    use pwf_models::task::{TaskId, TaskStatus};

    use super::*;

    fn sample_task() -> TaskView {
        TaskView {
            id: "PWF-0064".to_string(),
            project: "pwf".to_string(),
            status: TaskStatus::Active,
            session: "make list commands formatting less redundant".to_string(),
            prompt: String::new(),
            repo: None,
            note: "pwf.md".to_string(),
            task_file: None,
            line: 1,
            format: String::new(),
            launchable: true,
            needs_prompt: false,
            issues: Vec::new(),
            section: None,
            prereq: None,
            prerequisite_statuses: Vec::new(),
            effort: None,
            tags: None,
            created: None,
        }
    }

    fn render_task_for_filter(
        task: &TaskView,
        status_filter: StatusFilter,
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
            task.status = status;
            let output = render_task_for_filter(&task, StatusFilter::All, false, false);
            assert_eq!(output, expected);
            assert!(!output.contains('\u{1b}'), "{output}");
        }
    }

    #[test]
    fn exact_status_short_lines_keep_the_existing_shape() {
        let output = render_task_for_filter(
            &sample_task(),
            StatusFilter::Exact(TaskStatus::Active),
            false,
            false,
        );
        assert_eq!(
            output,
            "PWF-0064 :: make list commands formatting less redundant"
        );
    }

    #[test]
    fn long_form_separates_lifecycle_from_active_launch_readiness() {
        let output = render_task_for_filter(
            &sample_task(),
            StatusFilter::Exact(TaskStatus::Active),
            true,
            false,
        );

        assert!(output.contains("  status: active\n"), "{output}");
        assert!(output.contains("  launch: READY\n"), "{output}");
    }

    #[test]
    fn closed_long_form_omits_active_launch_diagnostics() {
        let mut task = sample_task();
        task.status = TaskStatus::Done;
        task.launchable = false;
        task.needs_prompt = true;
        task.issues = vec!["missing repository".to_string()];

        let output =
            render_task_for_filter(&task, StatusFilter::Exact(TaskStatus::Done), true, false);

        assert!(output.contains("  status: done\n"), "{output}");
        assert!(!output.contains("launch:"), "{output}");
        assert!(!output.contains("issue:"), "{output}");
        assert!(!output.contains("fix:"), "{output}");
    }

    #[test]
    fn long_form_formats_typed_prerequisite_statuses() {
        let mut task = sample_task();
        task.prerequisite_statuses = vec![
            PrerequisiteStatus {
                id: TaskId::try_new("CFG-0014").unwrap(),
                status: Some(TaskStatus::Done),
            },
            PrerequisiteStatus {
                id: TaskId::try_new("CFG-0015").unwrap(),
                status: Some(TaskStatus::Active),
            },
            PrerequisiteStatus {
                id: TaskId::try_new("CFG-9999").unwrap(),
                status: None,
            },
        ];

        let output = render_task_for_filter(&task, StatusFilter::default(), true, false);

        assert!(
            output.contains("  prereq: CFG-0014 (done), CFG-0015 (active), CFG-9999 (missing)\n"),
            "{output}"
        );
    }

    #[test]
    fn list_footer_mentions_hidden_count_and_escape_hatch() {
        let result = ListTasksOk {
            tasks: vec![sample_task()],
            hidden: 2,
            project: None,
            project_task_path: None,
            status_filter: StatusFilter::default(),
            grouped: false,
        };
        let output = render_list(
            &result,
            "notes",
            StatusFilter::default(),
            false,
            false,
            false,
        );

        assert!(
            output.ends_with("\n... and 2 more; use '--all' to list everything"),
            "got: {output}"
        );
    }
}
