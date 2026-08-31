mod active_task;
pub mod add_task;
mod blocked_by;
pub mod cancel_task;
pub mod complete_task;
pub mod edit_task;
pub mod get_task;
mod lane_configuration;
pub mod list_tasks;
pub mod migrate_task_metadata;
mod mutation_request;
mod note_body;
pub mod remove_task;
pub mod reopen_task;
pub mod resolve_task_project;
pub mod session;
mod tags;
mod task_closure;
mod task_creation;
mod task_view;

pub use lane_configuration::TaskPromptLanesError;
pub use mutation_request::MutationRequestError;
pub use task_closure::CloseTaskError;

/// Reports a shorthand prompt without a usable leading task title.
#[derive(Debug, thiserror::Error)]
pub enum TaskPromptTitleError {
    #[error("--prompt must start with a nonempty title before any lane marker.")]
    Missing,
    #[error(transparent)]
    Invalid(#[from] pwf_models::task::TaskTitleError),
}

fn infer_task_title(
    prompt: &pwf_models::task::TaskPrompt,
    lanes: &lane_configuration::TaskPromptLanes,
) -> Result<pwf_models::task::TaskTitle, TaskPromptTitleError> {
    let (title, _) = lanes.parse(prompt.as_ref()).into_parts();
    if title.is_empty() {
        return Err(TaskPromptTitleError::Missing);
    }
    pwf_models::task::TaskTitle::try_new(title).map_err(Into::into)
}

fn task_body_region(body: &str) -> &str {
    body.strip_prefix('\n').unwrap_or(body)
}

fn task_revision(
    record: &crate::ports::task_record::TaskRecord,
) -> pwf_models::revision::ContentRevision {
    record.revision.clone()
}

#[derive(Debug, thiserror::Error)]
#[error(
    "task changed since it was read (expected revision {expected}, current revision {current})"
)]
pub struct TaskRevisionConflict {
    expected: pwf_models::revision::ContentRevision,
    current: pwf_models::revision::ContentRevision,
}

fn ensure_task_revision(
    expected: Option<&pwf_models::revision::ContentRevision>,
    record: &crate::ports::task_record::TaskRecord,
) -> Result<(), TaskRevisionConflict> {
    let Some(expected) = expected else {
        return Ok(());
    };
    let current = record.revision.clone();
    if expected == &current {
        return Ok(());
    }
    Err(TaskRevisionConflict {
        expected: expected.clone(),
        current,
    })
}

fn expected_task_revision(
    record: &crate::ports::task_record::TaskRecord,
) -> crate::ports::task_record::ExpectedTaskRevision {
    crate::ports::task_record::ExpectedTaskRevision {
        id: record.id.clone(),
        revision: record.revision.clone(),
    }
}

fn commit_task_writes(
    store: &impl crate::ports::task_record::TaskMutationStore,
    project: &pwf_models::project::Project,
    expected: Vec<crate::ports::task_record::ExpectedTaskRevision>,
    writes: Vec<crate::ports::task_record::TaskWrite>,
) -> Result<(), crate::ports::task_record::TaskMutationError<anyhow::Error>> {
    let writes = crate::ports::task_record::TaskWriteSet::try_new(expected, writes)
        .map_err(anyhow::Error::new)
        .map_err(crate::ports::task_record::TaskMutationError::Store)?;
    crate::ports::task_record::TaskMutationStore::commit_task_writes(store, project, writes)
        .map_err(|error| error.map_store(anyhow::Error::new))
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
