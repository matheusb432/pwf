use std::path::PathBuf;

use pwf_models::task::TaskId;

use super::resolve_task_project::{self, ResolveTaskProject, ResolveTaskProjectError};
use crate::ports::{
    confirmation::{Confirmation, ConfirmationClient},
    task_record::{IndexEntryStore, Materialization, TaskStore},
};

/// Describes the note and index link deleted by [`execute`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovedTask {
    /// Identifier of the deleted task.
    pub id: TaskId,
    /// Managed project that contained the task.
    pub project: String,
    /// Title of the deleted task.
    pub title: String,
    /// Path of the deleted task note.
    pub deleted_path: PathBuf,
    /// Index note from which the task link was removed.
    pub unlinked: String,
}

#[derive(Debug, Clone)]
pub struct RemoveTask {
    pub id: TaskId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoveTaskOk {
    Removed(RemovedTask),
    Aborted { task_identifier: TaskId },
}

#[derive(Debug, thiserror::Error)]
pub enum RemoveTaskError {
    #[error("Task not found: {id}")]
    TaskNotFound { id: TaskId },
    #[error(transparent)]
    ResolveProject(#[from] ResolveTaskProjectError),
    #[error("Task note missing: {path}")]
    NoteMissing { path: String },
    #[error("{0}")]
    WriteStore(Box<dyn std::error::Error + Send + Sync>),
}

/// Deletes a task after unlinking its index entry.
///
/// An unlink failure leaves the note untouched.
#[cqrsy::command]
pub async fn execute(
    cmd: &RemoveTask,
    store: &(impl TaskStore + IndexEntryStore),
    pool: &sqlx::SqlitePool,
    confirmation_client: &(impl ConfirmationClient + Send + Sync + 'static),
) -> Result<RemoveTaskOk, RemoveTaskError> {
    let project =
        resolve_task_project::execute(ResolveTaskProject { id: cmd.id.clone() }, pool).await?;
    let task_identifier = cmd.id.clone();
    let record = TaskStore::get(store, &project, &task_identifier)
        .map_err(|error| RemoveTaskError::WriteStore(Box::new(error)))?
        .ok_or_else(|| RemoveTaskError::TaskNotFound {
            id: task_identifier.clone(),
        })?;
    let note_path = match &record.materialization {
        Materialization::NoteFile => PathBuf::from(&record.locator),
        Materialization::MissingNote { expected } => {
            return Err(RemoveTaskError::NoteMissing {
                path: expected.clone(),
            });
        }
    };
    let confirmation = Confirmation::Removal {
        task_identifier: task_identifier.clone(),
        project: project.title.clone(),
        title: record.title.clone(),
        status: record.status,
        note_path: note_path.clone(),
    };
    if !confirmation_client.confirm(&confirmation) {
        return Ok(RemoveTaskOk::Aborted { task_identifier });
    }

    IndexEntryStore::delete_index_entry(store, &project, &task_identifier)
        .map_err(|error| RemoveTaskError::WriteStore(Box::new(error)))?;
    TaskStore::delete(store, &project, &task_identifier)
        .map_err(|error| RemoveTaskError::WriteStore(Box::new(error)))?;

    let removed = RemovedTask {
        id: task_identifier,
        project: project.title.to_string(),
        title: record.title,
        deleted_path: note_path,
        unlinked: record
            .placement
            .map(|placement| placement.index_path)
            .unwrap_or_default(),
    };
    Ok(RemoveTaskOk::Removed(removed))
}

#[cfg(test)]
mod tests {
    use pwf_models::task::{TaskId, TaskStatus, Timestamp};

    use super::{RemoveTask, RemoveTaskError, RemoveTaskOk};
    use crate::{
        ports::{
            confirmation::{Confirmation, ConfirmationClient},
            task_record::{
                IndexEntry, IndexEntryState, IndexEntryStore, Materialization, TaskRecord,
            },
        },
        testing::{InMemoryStore, insert_project, project, task_record},
    };

    async fn execute(
        command: &RemoveTask,
        store: &InMemoryStore,
        pool: &sqlx::SqlitePool,
        confirmation: &(impl ConfirmationClient + Send + Sync + 'static),
    ) -> Result<RemoveTaskOk, RemoveTaskError> {
        super::execute(command, store, pool, confirmation).await
    }

    fn record(id: &str, status: TaskStatus) -> TaskRecord {
        TaskRecord {
            title: "stale task".to_string(),
            status,
            created: Some(Timestamp::new("2026-07-01")),
            locator: format!("/notes/pwf/{id}.md"),
            ..task_record(id)
        }
    }

    fn staged(status: TaskStatus) -> InMemoryStore {
        let index_state = match status {
            TaskStatus::Active => IndexEntryState::Open,
            TaskStatus::Done | TaskStatus::Cancelled => {
                IndexEntryState::Done(Timestamp::new("2026-07-02"))
            }
        };
        let store = InMemoryStore::default()
            .with_project_id("pwf", "PWF")
            .with_project("pwf", vec![record("PWF-0001", status)]);
        IndexEntryStore::upsert_index_entry(
            &store,
            &project("PWF", "pwf"),
            IndexEntry {
                id: TaskId::try_new("PWF-0001").unwrap(),
                state: index_state,
                section: String::new(),
            },
        )
        .unwrap();
        store
    }

    fn command(id: &str) -> RemoveTask {
        RemoveTask {
            id: id.parse().unwrap(),
        }
    }

    #[derive(Clone)]
    struct Accepted;

    impl ConfirmationClient for Accepted {
        fn confirm(&self, _confirmation: &Confirmation) -> bool {
            true
        }
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn remove_deletes_record_and_index_entry(pool: sqlx::SqlitePool) {
        insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
        let store = staged(TaskStatus::Active);

        let RemoveTaskOk::Removed(removed) =
            execute(&command("PWF-0001"), &store, &pool, &Accepted)
                .await
                .unwrap()
        else {
            panic!("accepted removal must remove the task");
        };

        assert_eq!(removed.id.as_ref(), "PWF-0001");
        assert_eq!(removed.project, "pwf");
        assert_eq!(removed.title, "stale task");
        assert!(store.tasks("pwf").is_empty(), "record must be deleted");
        assert!(
            store.entries("pwf").is_empty(),
            "index entry must be unlinked"
        );
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn remove_deletes_an_unindexed_task(pool: sqlx::SqlitePool) {
        insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
        let store = InMemoryStore::default()
            .with_project_id("pwf", "PWF")
            .with_project("pwf", vec![record("PWF-0001", TaskStatus::Active)]);

        let outcome = execute(&command("PWF-0001"), &store, &pool, &Accepted)
            .await
            .unwrap();

        assert!(matches!(outcome, RemoveTaskOk::Removed(_)));
        assert!(store.tasks("pwf").is_empty());
        assert!(store.entries("pwf").is_empty());
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn remove_deletes_closed_items(pool: sqlx::SqlitePool) {
        insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
        for status in [TaskStatus::Done, TaskStatus::Cancelled] {
            let store = staged(status);

            let outcome = execute(&command("PWF-0001"), &store, &pool, &Accepted)
                .await
                .unwrap();

            assert!(matches!(outcome, RemoveTaskOk::Removed(_)));
            assert!(store.tasks("pwf").is_empty(), "{status} record retained");
            assert!(store.entries("pwf").is_empty(), "{status} index retained");
        }
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn remove_missing_item_preserves_requested_id(pool: sqlx::SqlitePool) {
        insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
        let store = staged(TaskStatus::Active);

        let error = execute(&command("PWF-9999"), &store, &pool, &Accepted)
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            RemoveTaskError::TaskNotFound { ref id } if id.as_ref() == "PWF-9999"
        ));
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn remove_reports_an_unknown_project_id(pool: sqlx::SqlitePool) {
        insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
        let store = staged(TaskStatus::Active);

        let error = execute(&command("XYZ-0001"), &store, &pool, &Accepted)
            .await
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "Unknown project ID `XYZ` for task XYZ-0001"
        );
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn remove_rejects_missing_note_wikilink_with_its_path(pool: sqlx::SqlitePool) {
        insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
        let ghost = TaskRecord {
            materialization: Materialization::MissingNote {
                expected: "/notes/pwf/PWF-0001.md".to_string(),
            },
            ..record("PWF-0001", TaskStatus::Active)
        };
        let store = InMemoryStore::default()
            .with_project_id("pwf", "PWF")
            .with_project("pwf", vec![ghost]);

        let error = execute(&command("PWF-0001"), &store, &pool, &Accepted)
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            RemoveTaskError::NoteMissing { ref path } if path == "/notes/pwf/PWF-0001.md"
        ));
        assert_eq!(
            error.to_string(),
            "Task note missing: /notes/pwf/PWF-0001.md"
        );
    }

    mod pwf_0144 {
        use super::{super::RemoveTaskOk, *};

        fn command(id: &str) -> RemoveTask {
            RemoveTask {
                id: id.parse().unwrap(),
            }
        }

        #[derive(Clone)]
        struct StaticInteraction {
            accepted: bool,
        }

        impl ConfirmationClient for StaticInteraction {
            fn confirm(&self, _confirmation: &Confirmation) -> bool {
                self.accepted
            }
        }

        #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
        async fn remove_deletes_record_and_index_entry_after_confirmation(pool: sqlx::SqlitePool) {
            insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
            let store = staged(TaskStatus::Active);

            let outcome = super::execute(
                &command("PWF-0001"),
                &store,
                &pool,
                &StaticInteraction { accepted: true },
            )
            .await
            .unwrap();
            let RemoveTaskOk::Removed(removed) = outcome else {
                panic!("accepted removal must remove the task");
            };

            assert_eq!(removed.id.as_ref(), "PWF-0001");
            assert!(store.tasks("pwf").is_empty());
            assert!(store.entries("pwf").is_empty());
        }

        #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
        async fn remove_decline_returns_aborted_without_mutating_task(pool: sqlx::SqlitePool) {
            insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
            let store = staged(TaskStatus::Active);

            let outcome = super::execute(
                &command("PWF-0001"),
                &store,
                &pool,
                &StaticInteraction { accepted: false },
            )
            .await
            .unwrap();

            assert_eq!(
                outcome,
                RemoveTaskOk::Aborted {
                    task_identifier: "PWF-0001".parse().unwrap(),
                }
            );
            assert_eq!(store.tasks("pwf").len(), 1);
            assert_eq!(store.entries("pwf").len(), 1);
        }
    }
}
