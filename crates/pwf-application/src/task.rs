pub mod activate_task;
mod active_task;
pub mod add_task;
pub mod backlog_task;
mod blocked_by;
pub mod cancel_task;
pub mod clone_task;
pub mod complete_task;
mod content;
pub mod edit_task;
pub mod get_task;
pub mod get_task_dag;
pub mod get_task_record;
pub mod list_tasks;
mod marker_sections;
pub mod read_task_dependencies;
pub mod remove_task;
pub mod resolve_task_project;
pub mod session;
mod task_closure;
mod task_projection;

pub use marker_sections::{TaskMarkerSectionRow, TaskMarkerSections, TaskMarkerSectionsError};
pub use task_closure::CloseTaskError;

/// Reports a shorthand body without a usable leading task title.
#[derive(Debug, thiserror::Error)]
pub enum TaskBodyTitleError {
    #[error("--body must start with a nonempty title before any section marker.")]
    Missing,
    #[error(transparent)]
    Invalid(#[from] pwf_models::task::TaskTitleError),
}

fn infer_task_title(
    body: &str,
    sections: &marker_sections::TaskMarkerSections,
) -> Result<pwf_models::task::TaskTitle, TaskBodyTitleError> {
    let (title, _) = sections.parse(body).into_parts();
    if title.is_empty() {
        return Err(TaskBodyTitleError::Missing);
    }
    pwf_models::task::TaskTitle::try_new(title).map_err(Into::into)
}

fn task_body_region(body: &str) -> &str {
    body.strip_prefix('\n').unwrap_or(body)
}

fn task_revision(record: &pwf_wire::task::TaskRecord) -> pwf_models::revision::ContentRevision {
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
    record: &pwf_wire::task::TaskRecord,
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
    record: &pwf_wire::task::TaskRecord,
) -> crate::ports::task_vault::ExpectedTaskRevision {
    crate::ports::task_vault::ExpectedTaskRevision {
        id: record.id.clone(),
        revision: record.revision.clone(),
    }
}

fn commit_task_writes(
    store: &impl crate::ports::task_vault::TaskVault,
    project: &pwf_models::project::Project,
    expected: Vec<crate::ports::task_vault::ExpectedTaskRevision>,
    writes: Vec<crate::ports::task_vault::TaskWrite>,
) -> Result<(), crate::ports::task_vault::TaskMutationError<anyhow::Error>> {
    let writes = crate::ports::task_vault::TaskWriteSet::try_new(expected, writes)
        .map_err(anyhow::Error::new)
        .map_err(crate::ports::task_vault::TaskMutationError::Store)?;
    crate::ports::task_vault::TaskVault::commit_task_writes(store, project, writes)
        .map_err(|error| error.map_store(anyhow::Error::new))
}
