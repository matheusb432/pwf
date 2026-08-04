pub mod add_task;
pub mod cancel_task;
mod close_task;
pub mod complete_task;
mod create_task;
pub mod find_active_task;
pub mod list_tasks;
mod note_body;
mod prerequisites;
pub mod remove_task;
pub mod reopen_task;
pub mod resolve_task_project;
pub mod session;
pub mod show_task;
mod tags;
mod task_view;
pub mod update_task;

pub use add_task::AddTaskOk;
pub use list_tasks::{
    ListMode, ListSection, ListTasksOk, OrderDirection, OrderField, OrderSpec, StatusFilter,
};
pub use remove_task::RemovedTask;
pub use show_task::ShowOutput;
pub use update_task::UpdateTaskOk;

/// Renders a prompt as Markdown while preserving placeholders and verbatim-authored prompts.
#[must_use]
pub fn note_body(prompt: &str) -> String {
    note_body::render(prompt)
}

fn normalize_commit_ranges(values: &[String]) -> Option<String> {
    let mut ranges = Vec::new();
    for range in values
        .iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|range| !range.is_empty())
    {
        if !ranges.contains(&range) {
            ranges.push(range);
        }
    }
    (!ranges.is_empty()).then(|| ranges.join(", "))
}

fn infer_task_title(
    prompt: &str,
) -> Result<pwf_models::task::TaskTitle, pwf_models::task::TaskTitleError> {
    pwf_models::task::TaskTitle::try_new(prompt_lanes::parse(prompt).title)
}

fn task_body_region(body: &str) -> &str {
    body.strip_prefix('\n').unwrap_or(body)
}

fn section_alias(label: &str) -> Option<&'static str> {
    match label.trim().to_ascii_lowercase().as_str() {
        "future" | "futuro" => Some("Future"),
        "human" => Some("Human"),
        "low-prio" | "low-priority" => Some("Low-prio"),
        _ => None,
    }
}

fn normalize_section_label(label: &str) -> String {
    section_alias(label).map_or_else(|| label.trim().to_string(), str::to_string)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TaskSection {
    Future,
    Human,
    LowPriority,
}

impl TaskSection {
    fn from_name(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "future" => Some(Self::Future),
            "human" => Some(Self::Human),
            "low-prio" => Some(Self::LowPriority),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Future => "Future",
            Self::Human => "Human",
            Self::LowPriority => "Low-prio",
        }
    }
}

fn created_task_output(
    project: &pwf_models::project::Project,
    created: create_task::CreatedTask,
) -> add_task::AddTaskOk {
    let id = created.record.id.clone();
    add_task::AddTaskOk {
        id,
        project: project.title.to_string(),
        title: created.record.title,
        note_path: std::path::PathBuf::from(created.record.locator),
        created_section: created.created_section,
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize_commit_ranges, normalize_section_label, section_alias};

    #[test]
    fn repeated_raw_ranges_are_normalized_in_first_seen_order() {
        let values = [" a..b,c..d ", "a..b", "", " e..f "]
            .map(str::to_string)
            .to_vec();

        assert_eq!(
            normalize_commit_ranges(&values).as_deref(),
            Some("a..b, c..d, e..f")
        );
        assert_eq!(
            normalize_commit_ranges(&[String::new(), "  , ".to_string()]),
            None
        );
    }

    #[test]
    fn section_aliases_map_only_known_labels() {
        assert_eq!(section_alias(" Futuro "), Some("Future"));
        assert_eq!(section_alias("future"), Some("Future"));
        assert_eq!(section_alias("HUMAN"), Some("Human"));
        assert_eq!(section_alias("low-priority"), Some("Low-prio"));
        assert_eq!(section_alias("Someday"), None);
        assert_eq!(normalize_section_label(" SomeDay "), "SomeDay");
    }
}
