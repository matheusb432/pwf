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

#[cfg(test)]
mod tests {
    use pwf_models::{
        project::Project,
        task::{TaskId, TaskTitle},
    };

    use super::{TaskCreation, create};
    use crate::{
        ports::task_vault::{IndexEntryState, NewTask},
        testing::{InMemoryStore, project, task_timestamp},
    };

    fn foo() -> Project {
        project("FOO", "foo")
    }

    fn new_task() -> NewTask {
        NewTask {
            body: "## Goals\n\n- do the thing".to_string(),
            title: TaskTitle::try_new("ship it").unwrap(),
            created_at: task_timestamp("2026-07-15T12:34:56Z"),
            blocked_by: None,
            effort: None,
            priority: None,
            tags: None,
        }
    }

    fn staged_store() -> InMemoryStore {
        InMemoryStore::default().with_project_id("foo", "FOO")
    }

    fn create_task(store: &InMemoryStore) {
        let project = foo();
        let id = TaskId::try_new("FOO-0001").unwrap();
        create(
            TaskCreation {
                project: &project,
                id: &id,
                new: new_task(),
            },
            store,
        )
        .unwrap();
    }

    #[test]
    fn create_task_inserts_record_and_open_index_entry() {
        let store = staged_store();

        create_task(&store);

        let id = TaskId::try_new("FOO-0001").unwrap();
        let tasks = store.tasks("foo");
        assert_eq!(tasks.len(), 1, "record must be inserted");
        assert_eq!(tasks[0].id, id.clone());
        let entries = store.entries("foo");
        assert_eq!(entries.len(), 1, "open index entry must be upserted");
        assert_eq!(entries[0].id, id);
        assert_eq!(entries[0].state, IndexEntryState::Open);
        assert_eq!(entries[0].section, None);
    }
}
