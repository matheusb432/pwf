use pwf_models::task::TaskId;

use super::{
    close_task::{self, CloseTask},
    complete_task::{CloseTaskError, ClosedTaskAction, CompleteTaskOk},
    resolve_task_project::{self, ResolveTaskProject, ResolveTaskProjectError},
};
use crate::ports::{
    clock::Clock,
    task_record::{IndexEntryStore, IndexSectionStore, TaskStore},
};

#[derive(Debug, Clone)]
pub struct CancelTask {
    pub id: TaskId,
    pub report: String,
    pub commits: Vec<String>,
    pub review: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum CancelTaskError {
    #[error(transparent)]
    ResolveProject(#[from] ResolveTaskProjectError),
    #[error(transparent)]
    Close(#[from] CloseTaskError),
}

#[cqrsy::command]
pub async fn execute(
    command: &CancelTask,
    store: &(impl TaskStore + IndexEntryStore + IndexSectionStore),
    pool: &sqlx::SqlitePool,
    clock: &impl Clock,
) -> Result<CompleteTaskOk, CancelTaskError> {
    let project = resolve_task_project::execute(
        ResolveTaskProject {
            id: command.id.clone(),
        },
        pool,
    )
    .await?;
    close_task::execute(
        CloseTask {
            action: ClosedTaskAction::Cancelled,
            id: &command.id,
            completed: clock.today(),
            report: Some(command.report.as_str()),
            commits: &command.commits,
            review: command.review,
        },
        store,
        &project,
    )
    .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use pwf_models::task::{TaskId, TaskStatus, Timestamp};

    use super::{CancelTask, CancelTaskError, CloseTaskError};
    use crate::{
        ports::task_record::{IndexEntry, IndexEntryState, IndexEntryStore, TaskRecord},
        testing::{FixedClock, InMemoryStore, project, task_record},
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
                section: String::new(),
            },
        )
        .unwrap();
        store
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn cancel_rejects_blank_report_during_execution(pool: sqlx::SqlitePool) {
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
            id: "FOO-0001".parse().unwrap(),
            report: " \t\n".to_string(),
            commits: Vec::new(),
            review: false,
        };

        let error = super::execute(&command, &staged(), &pool, &FixedClock)
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            CancelTaskError::Close(CloseTaskError::EmptyReport)
        ));
        assert_eq!(error.to_string(), "--report cannot be empty.");
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
            report: "obsoleted".to_string(),
            commits: vec![" a..b, c..d ".to_string(), "a..b".to_string()],
            review: false,
        };

        let out = super::execute(&command, &store, &pool, &FixedClock)
            .await
            .unwrap();

        assert_eq!(
            out.action,
            super::super::complete_task::ClosedTaskAction::Cancelled
        );
        assert_eq!(store.tasks("foo-bar")[0].status, TaskStatus::Cancelled);
        assert_eq!(
            store.tasks("foo-bar")[0].completed,
            Some(Timestamp::new("2026-07-26"))
        );
        assert_eq!(
            store.tasks("foo-bar")[0].commits.as_deref(),
            Some("a..b, c..d")
        );
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn cancel_uses_the_clock_date(pool: sqlx::SqlitePool) {
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
            report: "obsoleted".to_string(),
            commits: Vec::new(),
            review: false,
        };

        super::execute(&command, &store, &pool, &FixedClock)
            .await
            .unwrap();

        assert_eq!(
            store.tasks("foo-bar")[0].completed,
            Some(Timestamp::new("2026-07-26"))
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
            report: "obsolete".to_string(),
            commits: Vec::new(),
            review: false,
        };

        let error = super::execute(&command, &staged(), &pool, &FixedClock)
            .await
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "Unknown project ID `XYZ` for task XYZ-0001"
        );
    }
}
