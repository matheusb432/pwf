use pwf_models::task::TaskTimestampError;
use pwf_wire::task::{ClosedTaskAction, CompleteTask};

use super::{
    CloseTaskError,
    mutation_request::{self, MutationOperation, MutationRequestState, MutationStart},
    resolve_task_project::{self, ResolveTaskProjectError},
    task_closure::{self, TaskClosure},
};
use crate::ports::{
    clock::Clock,
    task_vault::{TaskMutationError, TaskVault},
};

#[derive(Debug, thiserror::Error)]
pub enum CompleteTaskError {
    #[error(transparent)]
    ResolveProject(#[from] ResolveTaskProjectError),
    #[error(transparent)]
    Close(#[from] CloseTaskError),
    #[error("cannot read the task completion time: {0}")]
    Clock(#[from] TaskTimestampError),
    #[error(transparent)]
    MutationRequest(#[from] mutation_request::MutationRequestError),
}

#[cqrsy::command]
pub async fn execute(
    command: &CompleteTask,
    store: &impl TaskVault,
    pool: &sqlx::SqlitePool,
    clock: &impl Clock,
) -> Result<(), CompleteTaskError> {
    let identity = mutation_request::identity(
        command.request_id.as_ref(),
        command.request_fingerprint.as_ref(),
    )?;
    if let Some(identity) = identity.as_ref()
        && let Some(replay) =
            mutation_request::find(pool, identity, MutationOperation::Complete).await?
    {
        return match replay.state {
            MutationRequestState::Completed => Ok(()),
            MutationRequestState::Pending => Err(identity.incomplete().into()),
        };
    }
    let project = resolve_task_project::execute(command.id.clone(), pool).await?;
    let completed_at = clock.now()?;
    if let Some(identity) = identity.as_ref()
        && let MutationStart::Existing(replay) =
            mutation_request::start(pool, identity, MutationOperation::Complete, &command.id)
                .await?
    {
        return match replay.state {
            MutationRequestState::Completed => Ok(()),
            MutationRequestState::Pending => Err(identity.incomplete().into()),
        };
    }
    let result = task_closure::close(
        &TaskClosure {
            action: ClosedTaskAction::Done,
            id: &command.id,
            completed_at,
            report: command.report.as_ref(),
            commits: command.commits.as_ref(),
            expected_revision: command.expected_revision.as_ref(),
        },
        store,
        &project,
    );
    match result {
        Ok(()) => {}
        Err(error) => {
            if let Some(identity) = identity.as_ref()
                && close_failed_before_mutation(&error)
            {
                mutation_request::discard(pool, identity, MutationOperation::Complete).await?;
            }
            return Err(error.into());
        }
    }
    if let Some(identity) = identity.as_ref() {
        mutation_request::complete(
            pool,
            identity,
            MutationOperation::Complete,
            Some("completed"),
        )
        .await?;
    }
    Ok(())
}

fn close_failed_before_mutation(error: &CloseTaskError) -> bool {
    matches!(
        error,
        CloseTaskError::TaskNotFound { .. }
            | CloseTaskError::UnknownProjectId { .. }
            | CloseTaskError::Revision(_)
            | CloseTaskError::Mutation(
                TaskMutationError::StaleTask { .. } | TaskMutationError::SourceChanged
            )
    )
}

/// Reports failures shared by the done and cancel interactors.
#[cfg(test)]
mod tests {
    use pwf_models::{
        project::Project,
        task::{TaskId, TaskStatus},
    };

    use super::{CloseTaskError, CompleteTask, CompleteTaskError};
    use crate::{
        ports::task_vault::{IndexEntry, IndexEntryState, TaskRecord, TaskVault},
        task::complete_task,
        testing::{FixedClock, InMemoryStore, project, task_record, task_timestamp},
    };

    fn record(id: &str, status: TaskStatus) -> TaskRecord {
        TaskRecord {
            status,
            ..task_record(id)
        }
    }

    fn entry(id: &str, state: IndexEntryState, section: &str) -> IndexEntry {
        IndexEntry {
            id: TaskId::try_new(id).unwrap(),
            state,
            section: (!section.is_empty()).then(|| section.parse().unwrap()),
        }
    }

    fn foo() -> Project {
        project("FOO", "foo-bar")
    }

    fn staged(tasks: Vec<TaskRecord>, entries: Vec<IndexEntry>) -> InMemoryStore {
        let store = InMemoryStore::default()
            .with_project_id("foo-bar", "FOO")
            .with_project("foo-bar", tasks);
        for entry in entries {
            TaskVault::upsert_index_entry(&store, &foo(), entry).unwrap();
        }
        store
    }

    fn done_command(id: &str) -> CompleteTask {
        CompleteTask {
            id: id.parse().unwrap(),
            report: None,
            commits: None,
            expected_revision: None,
            request_id: None,
            request_fingerprint: None,
        }
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn done_evicts_the_oldest_note_completion_when_index_dates_are_absent(
        pool: sqlx::SqlitePool,
    ) {
        crate::testing::insert_project(
            &pool,
            "FOO",
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let mut tasks = vec![record("FOO-0007", TaskStatus::Active)];
        let mut entries: Vec<IndexEntry> = (1..=6)
            .map(|n| {
                tasks.push(TaskRecord {
                    completed_at: Some(task_timestamp(format!("2026-01-{:02}T00:00:00Z", 7 - n))),
                    ..record(&format!("FOO-{n:04}"), TaskStatus::Done)
                });
                entry(&format!("FOO-{n:04}"), IndexEntryState::Done(None), "")
            })
            .collect();
        entries.push(entry("FOO-0007", IndexEntryState::Open, ""));
        let store = staged(tasks, entries);

        complete_task::execute(&done_command("FOO-0007"), &store, &pool, &FixedClock)
            .await
            .unwrap();

        assert_eq!(store.tasks("foo-bar")[0].status, TaskStatus::Done);
        let marked = store
            .entries("foo-bar")
            .into_iter()
            .find(|e| e.id == TaskId::try_new("FOO-0007").unwrap())
            .unwrap();
        assert_eq!(
            marked.state,
            IndexEntryState::Done(Some(task_timestamp("2026-07-26T12:34:56Z")))
        );
        assert!(
            !store
                .entries("foo-bar")
                .iter()
                .any(|e| e.id == TaskId::try_new("FOO-0006").unwrap()),
            "evicted entry must be unlinked"
        );
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn done_uses_the_clock_timestamp(pool: sqlx::SqlitePool) {
        crate::testing::insert_project(
            &pool,
            "FOO",
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let store = staged(
            vec![record("FOO-0001", TaskStatus::Active)],
            vec![entry("FOO-0001", IndexEntryState::Open, "")],
        );
        complete_task::execute(&done_command("FOO-0001"), &store, &pool, &FixedClock)
            .await
            .unwrap();

        assert_eq!(
            store.tasks("foo-bar")[0].completed_at,
            Some(task_timestamp("2026-07-26T12:34:56Z"))
        );
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn done_updates_an_unindexed_active_task(pool: sqlx::SqlitePool) {
        crate::testing::insert_project(
            &pool,
            "FOO",
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let store = staged(vec![record("FOO-0001", TaskStatus::Active)], Vec::new());

        complete_task::execute(&done_command("FOO-0001"), &store, &pool, &FixedClock)
            .await
            .unwrap();

        assert_eq!(store.tasks("foo-bar")[0].status, TaskStatus::Done);
        assert!(store.entries("foo-bar").is_empty());
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn done_on_missing_item_reports_item_not_found(pool: sqlx::SqlitePool) {
        crate::testing::insert_project(
            &pool,
            "FOO",
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let store = staged(Vec::new(), Vec::new());

        let error = complete_task::execute(&done_command("FOO-9999"), &store, &pool, &FixedClock)
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            CompleteTaskError::Close(CloseTaskError::TaskNotFound { ref id })
                if id.as_ref() == "FOO-9999"
        ));
        assert_eq!(error.to_string(), "Active task not found: FOO-9999");
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn done_does_not_validate_an_unreturned_persisted_title(pool: sqlx::SqlitePool) {
        crate::testing::insert_project(
            &pool,
            "FOO",
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let invalid = TaskRecord {
            title: "x".repeat(201),
            ..record("FOO-0001", TaskStatus::Active)
        };
        let store = staged(
            vec![invalid],
            vec![entry("FOO-0001", IndexEntryState::Open, "")],
        );

        complete_task::execute(&done_command("FOO-0001"), &store, &pool, &FixedClock)
            .await
            .unwrap();

        assert_eq!(store.tasks("foo-bar")[0].status, TaskStatus::Done);
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn done_reports_an_unknown_project_id(pool: sqlx::SqlitePool) {
        crate::testing::insert_project(
            &pool,
            "FOO",
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let store = staged(Vec::new(), Vec::new());

        let error = complete_task::execute(&done_command("XYZ-0001"), &store, &pool, &FixedClock)
            .await
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "Unknown project ID `XYZ` for task XYZ-0001"
        );
    }
}
