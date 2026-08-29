mod active_task;
pub mod add_task;
mod blocked_by;
pub mod cancel_task;
pub mod complete_task;
pub mod edit_task;
pub mod get_task;
pub mod list_tasks;
pub mod migrate_task_metadata;
mod note_body;
pub mod remove_task;
pub mod reopen_task;
pub mod resolve_task_project;
pub mod session;
mod tags;
mod task_closure;
mod task_creation;
mod task_view;

pub use task_closure::CloseTaskError;

fn infer_task_title(
    prompt: &pwf_models::task::TaskPrompt,
) -> Result<pwf_models::task::TaskTitle, pwf_models::task::TaskTitleError> {
    pwf_models::task::TaskTitle::try_new(prompt_lanes::parse(prompt.as_ref()).title)
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

fn normalize_section_label(label: &pwf_models::task::TaskSection) -> pwf_models::task::TaskSection {
    match section_alias(label.as_ref()) {
        Some("Future") => pwf_models::task::TaskSection::future(),
        Some("Human") => pwf_models::task::TaskSection::human(),
        Some("Low-prio") => pwf_models::task::TaskSection::low_priority(),
        Some(_) | None => label.clone(),
    }
}

fn created_task_output(
    project: &pwf_models::project::Project,
    created: task_creation::CreatedTask,
) -> pwf_wire::task::AddedTask {
    pwf_wire::task::AddedTask {
        id: created.id,
        project: project.title.clone(),
        title: created.title,
        note_path: created.note_path,
        created_section: created.created_section,
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize_section_label, section_alias};

    #[test]
    fn section_aliases_map_only_known_labels() {
        assert_eq!(section_alias(" Futuro "), Some("Future"));
        assert_eq!(section_alias("future"), Some("Future"));
        assert_eq!(section_alias("HUMAN"), Some("Human"));
        assert_eq!(section_alias("low-priority"), Some("Low-prio"));
        assert_eq!(section_alias("Someday"), None);
        let someday = " SomeDay ".parse().unwrap();
        assert_eq!(normalize_section_label(&someday).as_ref(), "SomeDay");
    }
}
