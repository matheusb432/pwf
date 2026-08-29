use pwf_models::task::TaskTimestampError;
use pwf_wire::task::{CancelTask, ClosedTask, ClosedTaskAction};

use super::{
    CloseTaskError,
    resolve_task_project::{self, ResolveTaskProjectError},
    task_closure::{self, TaskClosure},
};
use crate::ports::{
    clock::Clock,
    task_record::{IndexEntryStore, IndexSectionStore, TaskStore},
};

#[derive(Debug, thiserror::Error)]
pub enum CancelTaskError {
    #[error(transparent)]
    ResolveProject(#[from] ResolveTaskProjectError),
    #[error(transparent)]
    Close(#[from] CloseTaskError),
    #[error("cannot read the task cancellation time: {0}")]
    Clock(#[from] TaskTimestampError),
}

#[cqrsy::command]
pub async fn execute(
    command: &CancelTask,
    store: &(impl TaskStore + IndexEntryStore + IndexSectionStore),
    pool: &sqlx::SqlitePool,
    clock: &impl Clock,
) -> Result<ClosedTask, CancelTaskError> {
    let project = resolve_task_project::execute(command.id.clone(), pool).await?;
    task_closure::close(
        &TaskClosure {
            action: ClosedTaskAction::Cancelled,
            id: &command.id,
            completed_at: clock.now()?,
            report: Some(&command.report),
            commits: command.commits.as_ref(),
            review: command.review,
        },
        store,
        &project,
    )
    .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use pwf_models::task::{TaskId, TaskStatus};

    use super::CancelTask;
    use crate::{
        ports::task_record::{IndexEntry, IndexEntryState, IndexEntryStore, TaskRecord},
        task::cancel_task,
        testing::{FixedClock, InMemoryStore, project, task_record, task_timestamp},
    };

    fn record(id: &str) -> TaskRecord {
        task_record(id)
    }

    fn staged() -> InMemoryStore {
        let store = InMemoryStore::default()
            .with_project_id("foo-bar", "FOO")
            .with_project("foo-bar", vec![record("FOO-0001")]);
        IndexEntryStore::upsert_index_entry(
            &store,
            &project("FOO", "foo-bar"),
            IndexEntry {
                id: TaskId::try_new("FOO-0001").unwrap(),
                state: IndexEntryState::Open,
                section: None,
            },
        )
        .unwrap();
        store
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn cancel_marks_item_cancelled(pool: sqlx::SqlitePool) {
        crate::testing::insert_project(
            &pool,
            "FOO",
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let store = staged();
        let command = CancelTask {
            id: "FOO-0001".parse().unwrap(),
            report: "obsoleted".parse().unwrap(),
            commits: "a..b, c..d".parse().ok(),
            review: false,
        };

        let out = cancel_task::execute(&command, &store, &pool, &FixedClock)
            .await
            .unwrap();

        assert_eq!(out.action, pwf_wire::task::ClosedTaskAction::Cancelled);
        assert_eq!(store.tasks("foo-bar")[0].status, TaskStatus::Cancelled);
        assert_eq!(
            store.tasks("foo-bar")[0].completed_at,
            Some(task_timestamp("2026-07-26T12:34:56Z"))
        );
        assert_eq!(
            store.tasks("foo-bar")[0].commits.as_deref(),
            Some("a..b, c..d")
        );
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn cancel_uses_the_clock_timestamp(pool: sqlx::SqlitePool) {
        crate::testing::insert_project(
            &pool,
            "FOO",
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let store = staged();
        let command = CancelTask {
            id: "FOO-0001".parse().unwrap(),
            report: "obsoleted".parse().unwrap(),
            commits: None,
            review: false,
        };

        cancel_task::execute(&command, &store, &pool, &FixedClock)
            .await
            .unwrap();

        assert_eq!(
            store.tasks("foo-bar")[0].completed_at,
            Some(task_timestamp("2026-07-26T12:34:56Z"))
        );
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn cancel_reports_an_unknown_project_id(pool: sqlx::SqlitePool) {
        crate::testing::insert_project(
            &pool,
            "FOO",
            "foo-bar",
            "/projects/foo",
            "/tasks/foo",
            false,
        )
        .await;
        let command = CancelTask {
            id: "XYZ-0001".parse().unwrap(),
            report: "obsolete".parse().unwrap(),
            commits: None,
            review: false,
        };

        let error = cancel_task::execute(&command, &staged(), &pool, &FixedClock)
            .await
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "Unknown project ID `XYZ` for task XYZ-0001"
        );
    }
}
