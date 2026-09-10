//! Creates a task record and its open index entry.

use pwf_models::{project::Project, task::TaskId};

use crate::ports::task_vault::{IndexEntry, IndexEntryState, NewTask, TaskVault};

/// Reports the persistence phase that failed while creating a task.
#[derive(Debug, thiserror::Error)]
pub enum CreateTaskError {
    #[error(transparent)]
    InsertRecord(anyhow::Error),
    #[error(transparent)]
    InsertIndex(anyhow::Error),
}

pub(in crate::task) struct TaskCreation<'a> {
    pub(in crate::task) project: &'a Project,
    pub(in crate::task) id: &'a TaskId,
    pub(in crate::task) new: NewTask,
}

/// Inserts a note record, then upserts its open index entry.
pub(in crate::task) fn create(
    command: TaskCreation<'_>,
    store: &impl TaskVault,
) -> Result<(), CreateTaskError> {
    let TaskCreation { project, id, new } = command;
    TaskVault::insert_task(store, project, id, new)
        .map_err(|error| CreateTaskError::InsertRecord(anyhow::Error::new(error)))?;
    upsert_index(store, project, id)
}

/// Finishes a previously reserved creation whose note is already observable.
pub(in crate::task) fn ensure_index(
    store: &impl TaskVault,
    project: &Project,
    id: &TaskId,
) -> Result<(), CreateTaskError> {
    upsert_index(store, project, id)
}

fn upsert_index(
    store: &impl TaskVault,
    project: &Project,
    id: &TaskId,
) -> Result<(), CreateTaskError> {
    TaskVault::upsert_index_entry(
        store,
        project,
        IndexEntry {
            id: id.clone(),
            state: IndexEntryState::Open,
            section: None,
        },
    )
    .map_err(|error| CreateTaskError::InsertIndex(anyhow::Error::new(error)))
}
