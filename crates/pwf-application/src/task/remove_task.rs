use pwf_models::task::{TaskId, TaskTitle, TaskTitleError};
use pwf_wire::{
    confirmation::{Confirmation, RemoveTaskConfirmation},
    project::{ListProjects, ProjectStatusFilter},
    task::{RemoveTask, RemovedTask, RemovedTaskOutcome, ResolveTaskProject, TaskNotePath},
};

use super::{
    blocked_by,
    resolve_task_project::{self, ResolveTaskProjectError},
};
use crate::{
    ports::{
        confirmation::ConfirmationClient,
        task_record::{IndexEntryStore, Materialization, StoredBlockedBy, TaskStore},
    },
    project::list_projects,
};

#[derive(Debug, thiserror::Error)]
pub enum RemoveTaskError {
    #[error("Task not found: {id}")]
    TaskNotFound { id: TaskId },
    #[error(transparent)]
    ResolveProject(#[from] ResolveTaskProjectError),
    #[error("Task note missing: {path}")]
    NoteMissing { path: TaskNotePath },
    #[error("task {id} has an invalid persisted title: {source}")]
    InvalidTitle {
        id: TaskId,
        #[source]
        source: TaskTitleError,
    },
    #[error(
        "cannot remove task {target}; dependent task(s): {}",
        blocked_by::format_task_ids(dependents)
    )]
    HasDependents {
        target: TaskId,
        dependents: Vec<TaskId>,
    },
    #[error("task {task} at {path} has malformed blocked_by metadata {raw:?}: {reason}")]
    MalformedBlockedBy {
        task: TaskId,
        path: Box<TaskNotePath>,
        raw: Box<str>,
        reason: Box<str>,
    },
    #[error("cannot inspect task dependents: {0}")]
    ReadDependents(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("{0}")]
    WriteStore(#[source] Box<dyn std::error::Error + Send + Sync>),
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
) -> Result<RemovedTaskOutcome, RemoveTaskError> {
    let project =
        resolve_task_project::execute(ResolveTaskProject { id: cmd.id.clone() }, pool).await?;
    let task_identifier = cmd.id.clone();
    let record = TaskStore::get(store, &project, &task_identifier)
        .map_err(|error| RemoveTaskError::WriteStore(Box::new(error)))?
        .ok_or_else(|| RemoveTaskError::TaskNotFound {
            id: task_identifier.clone(),
        })?;
    let note_path = match &record.materialization {
        Materialization::NoteFile => record.locator.clone(),
        Materialization::MissingNote { expected } => {
            return Err(RemoveTaskError::NoteMissing {
                path: expected.clone(),
            });
        }
    };
    let title =
        TaskTitle::try_new(record.title).map_err(|source| RemoveTaskError::InvalidTitle {
            id: task_identifier.clone(),
            source,
        })?;
    let dependents = find_dependents(&task_identifier, store, pool).await?;
    if !dependents.is_empty() {
        return Err(RemoveTaskError::HasDependents {
            target: task_identifier,
            dependents,
        });
    }
    let confirmation = Confirmation::RemoveTask(RemoveTaskConfirmation {
        task_identifier: task_identifier.clone(),
        project: project.title.clone(),
        title: title.clone(),
        status: record.status,
        note_path: note_path.clone(),
    });
    if !confirmation_client.confirm(&confirmation) {
        return Ok(RemovedTaskOutcome::Aborted {
            task_id: task_identifier,
        });
    }

    IndexEntryStore::delete_index_entry(store, &project, &task_identifier)
        .map_err(|error| RemoveTaskError::WriteStore(Box::new(error)))?;
    TaskStore::delete(store, &project, &task_identifier)
        .map_err(|error| RemoveTaskError::WriteStore(Box::new(error)))?;

    let removed = RemovedTask {
        id: task_identifier,
        project: project.title,
        title,
        deleted_path: note_path,
        unlinked: record.placement.map(|placement| placement.index_path),
    };
    Ok(RemovedTaskOutcome::Removed(removed))
}

async fn find_dependents(
    target: &TaskId,
    store: &impl TaskStore,
    pool: &sqlx::SqlitePool,
) -> Result<Vec<TaskId>, RemoveTaskError> {
    let projects = list_projects::execute(
        ListProjects {
            status: ProjectStatusFilter::IncludingPaused,
        },
        pool,
    )
    .await
    .map_err(|error| RemoveTaskError::ReadDependents(Box::new(error)))?;
    let mut dependents = Vec::new();
    for project in projects {
        let records = store
            .list(&project)
            .map_err(|error| RemoveTaskError::ReadDependents(Box::new(error)))?;
        for record in records {
            match record.blocked_by {
                StoredBlockedBy::Valid(blocked_by)
                    if blocked_by.iter().any(|blocker| blocker == target) =>
                {
                    dependents.push(record.id);
                }
                StoredBlockedBy::Absent | StoredBlockedBy::Valid(_) => {}
                StoredBlockedBy::Malformed { raw, reason } => {
                    return Err(RemoveTaskError::MalformedBlockedBy {
                        task: record.id,
                        path: Box::new(record.locator),
                        raw: raw.into_boxed_str(),
                        reason: reason.into_boxed_str(),
                    });
                }
            }
        }
    }
    dependents.sort();
    dependents.dedup();
    Ok(dependents)
}

#[cfg(test)]
mod tests {
    use pwf_models::task::{TaskId, TaskStatus};
    use pwf_wire::{
        confirmation::Confirmation,
        task::{RemovedTaskOutcome, TaskNotePath},
    };

    use super::{RemoveTask, RemoveTaskError};
    use crate::{
        ports::{
            confirmation::ConfirmationClient,
            task_record::{
                IndexEntry, IndexEntryState, IndexEntryStore, Materialization, TaskRecord,
            },
        },
        task::remove_task,
        testing::{
            InMemoryStore, app_date, insert_project, project, stored_blocked_by, task_record,
        },
    };

    async fn run(
        command: &RemoveTask,
        store: &InMemoryStore,
        pool: &sqlx::SqlitePool,
        confirmation: &(impl ConfirmationClient + Send + Sync + 'static),
    ) -> Result<RemovedTaskOutcome, RemoveTaskError> {
        remove_task::execute(command, store, pool, confirmation).await
    }

    fn record(id: &str, status: TaskStatus) -> TaskRecord {
        TaskRecord {
            title: "stale task".to_string(),
            status,
            created: Some(app_date("2026-07-01")),
            locator: TaskNotePath::new(format!("/notes/pwf/{id}.md").into()),
            ..task_record(id)
        }
    }

    fn staged(status: TaskStatus) -> InMemoryStore {
        let index_state = match status {
            TaskStatus::Active => IndexEntryState::Open,
            TaskStatus::Done | TaskStatus::Cancelled => {
                IndexEntryState::Done(Some(app_date("2026-07-02")))
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
                section: None,
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

        let RemovedTaskOutcome::Removed(removed) =
            run(&command("PWF-0001"), &store, &pool, &Accepted)
                .await
                .unwrap()
        else {
            panic!("accepted removal must remove the task");
        };

        assert_eq!(removed.id.as_ref(), "PWF-0001");
        assert_eq!(removed.project.as_ref(), "pwf");
        assert_eq!(removed.title.as_ref(), "stale task");
        assert!(store.tasks("pwf").is_empty(), "record must be deleted");
        assert!(
            store.entries("pwf").is_empty(),
            "index entry must be unlinked"
        );
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn remove_reports_all_dependents_including_paused_projects_without_mutating(
        pool: sqlx::SqlitePool,
    ) {
        insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
        insert_project(
            &pool,
            "AUX",
            "paused-project",
            "/projects/paused",
            "/tasks/paused",
            true,
        )
        .await;
        let local_dependent = TaskRecord {
            blocked_by: stored_blocked_by(&["PWF-0001"]),
            ..record("PWF-0003", TaskStatus::Done)
        };
        let paused_dependent = TaskRecord {
            blocked_by: stored_blocked_by(&["PWF-0001"]),
            ..record("AUX-0002", TaskStatus::Active)
        };
        let store = staged(TaskStatus::Active)
            .with_project(
                "pwf",
                vec![record("PWF-0001", TaskStatus::Active), local_dependent],
            )
            .with_project("paused-project", vec![paused_dependent]);
        let before = store.tasks("pwf");

        let error = run(&command("PWF-0001"), &store, &pool, &Accepted)
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            RemoveTaskError::HasDependents { ref target, ref dependents }
                if target.as_ref() == "PWF-0001"
                    && dependents.iter().map(AsRef::as_ref).collect::<Vec<_>>()
                        == ["AUX-0002", "PWF-0003"]
        ));
        assert_eq!(store.tasks("pwf"), before);
        assert_eq!(store.entries("pwf").len(), 1);
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn remove_rejects_an_invalid_persisted_title_before_mutation(pool: sqlx::SqlitePool) {
        insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
        let store = InMemoryStore::default()
            .with_project_id("pwf", "PWF")
            .with_project(
                "pwf",
                vec![TaskRecord {
                    title: "x".repeat(201),
                    ..record("PWF-0001", TaskStatus::Active)
                }],
            );

        let error = run(&command("PWF-0001"), &store, &pool, &Accepted)
            .await
            .unwrap_err();

        assert!(matches!(error, RemoveTaskError::InvalidTitle { .. }));
        assert_eq!(store.tasks("pwf").len(), 1);
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn remove_deletes_an_unindexed_task(pool: sqlx::SqlitePool) {
        insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
        let store = InMemoryStore::default()
            .with_project_id("pwf", "PWF")
            .with_project("pwf", vec![record("PWF-0001", TaskStatus::Active)]);

        let outcome = run(&command("PWF-0001"), &store, &pool, &Accepted)
            .await
            .unwrap();

        assert!(matches!(outcome, RemovedTaskOutcome::Removed(_)));
        assert!(store.tasks("pwf").is_empty());
        assert!(store.entries("pwf").is_empty());
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn remove_deletes_closed_items(pool: sqlx::SqlitePool) {
        insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
        for status in [TaskStatus::Done, TaskStatus::Cancelled] {
            let store = staged(status);

            let outcome = run(&command("PWF-0001"), &store, &pool, &Accepted)
                .await
                .unwrap();

            assert!(matches!(outcome, RemovedTaskOutcome::Removed(_)));
            assert!(store.tasks("pwf").is_empty(), "{status} record retained");
            assert!(store.entries("pwf").is_empty(), "{status} index retained");
        }
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn remove_missing_item_preserves_requested_id(pool: sqlx::SqlitePool) {
        insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
        let store = staged(TaskStatus::Active);

        let error = run(&command("PWF-9999"), &store, &pool, &Accepted)
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

        let error = run(&command("XYZ-0001"), &store, &pool, &Accepted)
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
                expected: TaskNotePath::new("/notes/pwf/PWF-0001.md".into()),
            },
            ..record("PWF-0001", TaskStatus::Active)
        };
        let store = InMemoryStore::default()
            .with_project_id("pwf", "PWF")
            .with_project("pwf", vec![ghost]);

        let error = run(&command("PWF-0001"), &store, &pool, &Accepted)
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            RemoveTaskError::NoteMissing { ref path }
                if path.as_path() == std::path::Path::new("/notes/pwf/PWF-0001.md")
        ));
        assert_eq!(
            error.to_string(),
            "Task note missing: /notes/pwf/PWF-0001.md"
        );
    }

    mod pwf_0144 {
        use pwf_wire::task::RemovedTaskOutcome;

        use super::*;

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

            let outcome = remove_task::execute(
                &command("PWF-0001"),
                &store,
                &pool,
                &StaticInteraction { accepted: true },
            )
            .await
            .unwrap();
            let RemovedTaskOutcome::Removed(removed) = outcome else {
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

            let outcome = remove_task::execute(
                &command("PWF-0001"),
                &store,
                &pool,
                &StaticInteraction { accepted: false },
            )
            .await
            .unwrap();

            assert_eq!(
                outcome,
                RemovedTaskOutcome::Aborted {
                    task_id: "PWF-0001".parse().unwrap(),
                }
            );
            assert_eq!(store.tasks("pwf").len(), 1);
            assert_eq!(store.entries("pwf").len(), 1);
        }
    }
}
