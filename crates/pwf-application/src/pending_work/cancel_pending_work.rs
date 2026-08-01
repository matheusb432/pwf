use pwf_models::project::Project;

use super::{
    add_pending_work_item::AddPendingWorkError,
    complete_pending_work::{
        ClosedItemAction, CompletePendingWorkError, CompletePendingWorkOk, active_project_for_item,
    },
    logic::pending_work_closing::{CloseError, perform_close},
};
use crate::ports::{
    clock::Clock,
    pending_work_record::{IndexEntryStore, IndexSectionStore, PendingWorkStore},
};

#[derive(Debug, Clone)]
pub struct CancelPendingWork {
    pub id: String,
    pub date: Option<String>,
    report: String,
    pub commits: Vec<String>,
    pub review: bool,
}

impl CancelPendingWork {
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
pub enum CancelPendingWorkError {
    #[error("--report cannot be empty.")]
    EmptyReport,
    #[error("Open pending-work item not found: {id}")]
    ItemNotFound { id: String },
    #[error("Unknown task id prefix `{prefix}` for {pending_work_identifier}")]
    UnknownPrefix {
        pending_work_identifier: String,
        prefix: String,
    },
    #[error("{0}")]
    WriteStore(Box<dyn std::error::Error + Send + Sync>),
    #[error("{0}")]
    ReviewTask(#[source] AddPendingWorkError),
    #[error("{0}")]
    QueryProject(#[source] Box<dyn std::error::Error + Send + Sync>),
}

#[cqrsy::command]
pub async fn execute(
    command: &CancelPendingWork,
    store: &(impl PendingWorkStore + IndexEntryStore + IndexSectionStore),
    pool: &sqlx::SqlitePool,
    clock: &impl Clock,
) -> Result<CompletePendingWorkOk, CancelPendingWorkError> {
    let project = active_project_for_item(&command.id, pool)
        .await
        .map_err(map_project_error)?;
    execute_with_project(command, store, &project, clock)
}

fn execute_with_project(
    command: &CancelPendingWork,
    store: &(impl PendingWorkStore + IndexEntryStore + IndexSectionStore),
    project: &Project,
    clock: &impl Clock,
) -> Result<CompletePendingWorkOk, CancelPendingWorkError> {
    let authored_date = command
        .date
        .clone()
        .map_or_else(|| clock.today(), pwf_models::pending_work::Timestamp::new);
    perform_close(
        store,
        project,
        ClosedItemAction::Cancelled,
        &command.id,
        authored_date.as_str(),
        Some(command.report.as_str()),
        &command.commits,
        command.review,
    )
    .map_err(map_close_error)
}

fn map_project_error(error: CompletePendingWorkError) -> CancelPendingWorkError {
    match error {
        CompletePendingWorkError::ItemNotFound { id } => {
            CancelPendingWorkError::ItemNotFound { id }
        }
        CompletePendingWorkError::UnknownPrefix {
            pending_work_identifier,
            prefix,
        } => CancelPendingWorkError::UnknownPrefix {
            pending_work_identifier,
            prefix,
        },
        CompletePendingWorkError::QueryProject(source) => {
            CancelPendingWorkError::QueryProject(source)
        }
        CompletePendingWorkError::EmptyReport
        | CompletePendingWorkError::WriteStore(_)
        | CompletePendingWorkError::ReviewTask(_) => {
            unreachable!("project lookup returns only identifier and query failures")
        }
    }
}

fn map_close_error(error: CloseError) -> CancelPendingWorkError {
    match error {
        CloseError::ItemNotFound { id } => CancelPendingWorkError::ItemNotFound { id },
        CloseError::UnknownPrefix {
            pending_work_identifier,
            prefix,
        } => CancelPendingWorkError::UnknownPrefix {
            pending_work_identifier,
            prefix,
        },
        CloseError::EmptyReport => CancelPendingWorkError::EmptyReport,
        CloseError::WriteStore(source) => CancelPendingWorkError::WriteStore(source),
        CloseError::ReviewTask(source) => CancelPendingWorkError::ReviewTask(source),
    }
}

#[cfg(test)]
mod tests {
    use pwf_models::{
        pending_work::{Timestamp, WorkItemId, WorkItemStatus},
        project::Project,
    };

    use super::{CancelPendingWork, CancelPendingWorkError};
    use crate::{
        ports::{
            clock::Clock,
            pending_work_record::{
                IndexEntry, IndexEntryState, IndexEntryStore, Materialization, PendingWorkRecord,
                RecordId,
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

    fn registry() -> Project {
        project("FOO", "foo-bar")
    }

    fn record(id: &str) -> PendingWorkRecord {
        PendingWorkRecord {
            id: RecordId::Item(WorkItemId::try_new(id).unwrap()),
            title: "tray gui".to_string(),
            status: WorkItemStatus::Active,
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
                id: WorkItemId::try_new("FOO-0001").unwrap(),
                state: IndexEntryState::Open,
                section: String::new(),
            },
        )
        .unwrap();
        store
    }

    #[test]
    fn cancel_rejects_blank_report_during_execution() {
        let command = CancelPendingWork::new(
            "FOO-0001".to_string(),
            Some("2026-07-14".to_string()),
            " \t\n".to_string(),
            Vec::new(),
            false,
        );

        let error =
            super::execute_with_project(&command, &staged(), &registry(), &FixedClock).unwrap_err();

        assert!(matches!(error, CancelPendingWorkError::EmptyReport));
        assert_eq!(error.to_string(), "--report cannot be empty.");
    }

    #[test]
    fn cancel_marks_item_cancelled() {
        let store = staged();
        let command = CancelPendingWork::new(
            "FOO-0001".to_string(),
            Some("2026-07-14".to_string()),
            "obsoleted".to_string(),
            vec![" a..b, c..d ".to_string(), "a..b".to_string()],
            false,
        );

        let out = super::execute_with_project(&command, &store, &registry(), &FixedClock).unwrap();

        assert_eq!(
            out.action,
            super::super::complete_pending_work::ClosedItemAction::Cancelled
        );
        assert_eq!(store.items("foo-bar")[0].status, WorkItemStatus::Cancelled);
        assert_eq!(
            store.items("foo-bar")[0].completed,
            Some(Timestamp::new("2026-07-14"))
        );
        assert_eq!(
            store.items("foo-bar")[0].commits.as_deref(),
            Some("a..b, c..d")
        );
    }

    #[test]
    fn cancel_uses_clock_date_when_no_date_is_explicit() {
        let store = staged();
        let command = CancelPendingWork::new(
            "FOO-0001".to_string(),
            None,
            "obsoleted".to_string(),
            Vec::new(),
            false,
        );

        super::execute_with_project(&command, &store, &registry(), &FixedClock).unwrap();

        assert_eq!(
            store.items("foo-bar")[0].completed,
            Some(Timestamp::new("2026-07-26"))
        );
    }

    #[test]
    fn cancel_reports_an_unknown_configured_prefix() {
        let command = CancelPendingWork::new(
            "XYZ-0001".to_string(),
            Some("2026-07-14".to_string()),
            "obsolete".to_string(),
            Vec::new(),
            false,
        );

        let error =
            super::execute_with_project(&command, &staged(), &registry(), &FixedClock).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Unknown task id prefix `XYZ` for XYZ-0001"
        );
    }
}
