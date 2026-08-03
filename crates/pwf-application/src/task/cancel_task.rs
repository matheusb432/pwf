use super::{
    add_task::AddTaskError,
    complete_task::{ClosedTaskAction, CompleteTaskOk},
    logic::task_closing::{CloseError, perform_close},
    resolve_task_project::{self, ResolveTaskProject, ResolveTaskProjectError},
};
use crate::ports::{
    clock::Clock,
    task_record::{IndexEntryStore, IndexSectionStore, TaskStore},
};

#[derive(Debug, Clone)]
pub struct CancelTask {
    pub id: String,
    pub date: Option<String>,
    report: String,
    pub commits: Vec<String>,
    pub review: bool,
}

impl CancelTask {
    pub fn new(
        id: String,
        date: Option<String>,
        report: String,
        commits: Vec<String>,
        review: bool,
    ) -> Self {
        Self {
            id,
            date,
            report,
            commits,
            review,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CancelTaskError {
    #[error("--report cannot be empty.")]
    EmptyReport,
    #[error("Active task not found: {id}")]
    TaskNotFound { id: String },
    #[error("Unknown task id prefix `{prefix}` for {task_identifier}")]
    UnknownPrefix {
        task_identifier: String,
        prefix: String,
    },
    #[error("{0}")]
    WriteStore(Box<dyn std::error::Error + Send + Sync>),
    #[error("{0}")]
    ReviewTask(#[source] AddTaskError),
    #[error("{0}")]
    QueryProject(#[source] Box<dyn std::error::Error + Send + Sync>),
}

#[cqrsy::command]
pub async fn execute(
    command: &CancelTask,
    store: &(impl TaskStore + IndexEntryStore + IndexSectionStore),
    pool: &sqlx::SqlitePool,
    clock: &impl Clock,
) -> Result<CompleteTaskOk, CancelTaskError> {
    let resolved = resolve_task_project::execute(
        ResolveTaskProject {
            id: command.id.clone(),
        },
        pool,
    )
    .await
    .map_err(map_project_error)?;
    let authored_date = command
        .date
        .clone()
        .map_or_else(|| clock.today(), pwf_models::task::Timestamp::new);
    perform_close(
        store,
        &resolved.project,
        ClosedTaskAction::Cancelled,
        &command.id,
        authored_date.as_str(),
        Some(command.report.as_str()),
        &command.commits,
        command.review,
    )
    .map_err(map_close_error)
}

fn map_project_error(error: ResolveTaskProjectError) -> CancelTaskError {
    match error {
        ResolveTaskProjectError::TaskNotFound { id } => CancelTaskError::TaskNotFound { id },
        ResolveTaskProjectError::UnknownPrefix {
            task_identifier,
            prefix,
        } => CancelTaskError::UnknownPrefix {
            task_identifier,
            prefix,
        },
        ResolveTaskProjectError::QueryProject(source) => CancelTaskError::QueryProject(source),
    }
}

fn map_close_error(error: CloseError) -> CancelTaskError {
    match error {
        CloseError::TaskNotFound { id } => CancelTaskError::TaskNotFound { id },
        CloseError::UnknownPrefix {
            task_identifier,
            prefix,
        } => CancelTaskError::UnknownPrefix {
            task_identifier,
            prefix,
        },
        CloseError::EmptyReport => CancelTaskError::EmptyReport,
        CloseError::WriteStore(source) => CancelTaskError::WriteStore(source),
        CloseError::ReviewTask(source) => CancelTaskError::ReviewTask(source),
    }
}

#[cfg(test)]
mod tests {
    use pwf_models::task::{TaskId, TaskStatus, Timestamp};

    use super::{CancelTask, CancelTaskError};
    use crate::{
        ports::{
            clock::Clock,
            task_record::{
                IndexEntry, IndexEntryState, IndexEntryStore, Materialization, RecordId, TaskRecord,
            },
        },
        testing::{InMemoryStore, project},
    };

    #[derive(Clone)]
    struct FixedClock;

    impl Clock for FixedClock {
        fn today(&self) -> Timestamp {
            Timestamp::new("2026-07-26")
        }
    }

    fn record(id: &str) -> TaskRecord {
        TaskRecord {
            id: RecordId::Task(TaskId::try_new(id).unwrap()),
            title: "tray gui".to_string(),
            status: TaskStatus::Active,
            created: Some(Timestamp::new("2026-01-01")),
            completed: None,
            commits: None,
            tags: None,
            effort: None,
            prereq: None,
            section: None,
            body: "\nbody\n".to_string(),
            source: "body".to_string(),
            locator: format!("/mem/foo-bar/{id}.md"),
            placement: None,
            materialization: Materialization::NoteFile,
        }
    }

    fn staged() -> InMemoryStore {
        let store = InMemoryStore::default()
            .with_prefix("foo-bar", "FOO")
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
        crate::testing::insert_project(&pool, "FOO", "foo-bar", "/repo/foo", "/tasks/foo", false)
            .await;
        let command = CancelTask::new(
            "FOO-0001".to_string(),
            Some("2026-07-14".to_string()),
            " \t\n".to_string(),
            Vec::new(),
            false,
        );

        let error = super::execute(&command, &staged(), &pool, &FixedClock)
            .await
            .unwrap_err();

        assert!(matches!(error, CancelTaskError::EmptyReport));
        assert_eq!(error.to_string(), "--report cannot be empty.");
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn cancel_marks_item_cancelled(pool: sqlx::SqlitePool) {
        crate::testing::insert_project(&pool, "FOO", "foo-bar", "/repo/foo", "/tasks/foo", false)
            .await;
        let store = staged();
        let command = CancelTask::new(
            "FOO-0001".to_string(),
            Some("2026-07-14".to_string()),
            "obsoleted".to_string(),
            vec![" a..b, c..d ".to_string(), "a..b".to_string()],
            false,
        );

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
            Some(Timestamp::new("2026-07-14"))
        );
        assert_eq!(
            store.tasks("foo-bar")[0].commits.as_deref(),
            Some("a..b, c..d")
        );
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn cancel_uses_clock_date_when_no_date_is_explicit(pool: sqlx::SqlitePool) {
        crate::testing::insert_project(&pool, "FOO", "foo-bar", "/repo/foo", "/tasks/foo", false)
            .await;
        let store = staged();
        let command = CancelTask::new(
            "FOO-0001".to_string(),
            None,
            "obsoleted".to_string(),
            Vec::new(),
            false,
        );

        super::execute(&command, &store, &pool, &FixedClock)
            .await
            .unwrap();

        assert_eq!(
            store.tasks("foo-bar")[0].completed,
            Some(Timestamp::new("2026-07-26"))
        );
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn cancel_reports_an_unknown_configured_prefix(pool: sqlx::SqlitePool) {
        crate::testing::insert_project(&pool, "FOO", "foo-bar", "/repo/foo", "/tasks/foo", false)
            .await;
        let command = CancelTask::new(
            "XYZ-0001".to_string(),
            Some("2026-07-14".to_string()),
            "obsolete".to_string(),
            Vec::new(),
            false,
        );

        let error = super::execute(&command, &staged(), &pool, &FixedClock)
            .await
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "Unknown task id prefix `XYZ` for XYZ-0001"
        );
    }
}
