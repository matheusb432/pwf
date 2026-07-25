use super::{
    add::AddPendingWorkError,
    done::{CloseError, ClosedItemAction, CompletedPendingWork, perform_close},
    project_registry::ProjectRegistry,
};
use crate::{
    HandoffDocumentStore, HandoffLedger,
    handoff::HandoffError,
    ports::{AppRecordStore, Clock, IndexEntry, IndexSection, PendingWorkItem},
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
    /// Reports a canonical identifier whose prefix has no configured project.
    #[error("Unknown task id prefix `{prefix}` for {pending_work_identifier}")]
    UnknownPrefix {
        pending_work_identifier: String,
        prefix: String,
    },
    #[error("{0}")]
    WriteStore(Box<dyn std::error::Error + Send + Sync>),
    #[error("{0}")]
    ReviewTask(#[source] AddPendingWorkError),
    #[error(transparent)]
    HandoffPreflight(HandoffError),
    #[error("{source}")]
    HandoffAfterPendingWork {
        pending_work_identifier: pwf_domain::pending_work::WorkItemId,
        completed: Box<CompletedPendingWork>,
        #[source]
        source: HandoffError,
    },
}

#[cqrsy::command]
pub fn execute<S, C>(
    command: &CancelPendingWork,
    store: &S,
    projects: &ProjectRegistry,
    clock: &C,
) -> Result<CompletedPendingWork, CancelPendingWorkError>
where
    S: AppRecordStore<PendingWorkItem>
        + AppRecordStore<IndexEntry>
        + AppRecordStore<IndexSection>
        + HandoffDocumentStore
        + AppRecordStore<HandoffLedger>,
    C: Clock,
{
    let authored_date = command
        .date
        .clone()
        .map_or_else(|| clock.today(), pwf_domain::pending_work::Timestamp::new);
    perform_close(
        store,
        projects,
        ClosedItemAction::Cancelled,
        &command.id,
        authored_date.as_str(),
        Some(command.report.as_str()),
        &command.commits,
        command.review,
    )
    .map_err(map_close_error)
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
        CloseError::HandoffPreflight(source) => CancelPendingWorkError::HandoffPreflight(source),
        CloseError::HandoffAfterPendingWork {
            pending_work_identifier,
            completed,
            source,
        } => CancelPendingWorkError::HandoffAfterPendingWork {
            pending_work_identifier,
            completed,
            source,
        },
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::SystemTime};

    use pwf_domain::{
        handoff::HandoffStatus,
        pending_work::{ProjectName, Timestamp, WorkItemId, WorkItemStatus},
    };

    use super::{CancelPendingWork, CancelPendingWorkError, ProjectRegistry};
    use crate::{
        HandoffDocument, HandoffDocumentIdentifier, HandoffLocation, HandoffScope, IndexEntry,
        IndexEntryState, Materialization, PendingWorkItem, RecordId,
        ports::Clock,
        testing::{FailurePoint, InMemoryStore},
    };

    #[derive(Clone)]
    struct FixedClock;

    impl Clock for FixedClock {
        fn today(&self) -> Timestamp {
            Timestamp::new("2026-07-26")
        }
    }

    fn registry() -> ProjectRegistry {
        ProjectRegistry::new(vec![(
            ProjectName::try_new("glep-shimeji").unwrap(),
            Some("/repo".to_string()),
            Some("GLP".to_string()),
        )])
    }

    fn record(id: &str) -> PendingWorkItem {
        PendingWorkItem {
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
            locator: format!("/mem/glep-shimeji/{id}.md"),
            placement: None,
            materialization: Materialization::NoteFile,
        }
    }

    fn staged() -> InMemoryStore {
        let store = InMemoryStore::default()
            .with_prefix("glep-shimeji", "GLP")
            .with_project("glep-shimeji", vec![record("GLP-0001")]);
        <InMemoryStore as crate::AppRecordStore<IndexEntry>>::insert(
            &store,
            &ProjectName::try_new("glep-shimeji").unwrap(),
            IndexEntry {
                id: WorkItemId::try_new("GLP-0001").unwrap(),
                state: IndexEntryState::Open,
                section: String::new(),
            },
        )
        .unwrap();
        store
    }

    fn handoff() -> HandoffDocument {
        HandoffDocument {
            identifier: HandoffDocumentIdentifier {
                file_name: "2026-01-01-tray-gui.md".to_string(),
                location: HandoffLocation::Active,
            },
            location: HandoffLocation::Active,
            project: Some(ProjectName::try_new("glep-shimeji").unwrap()),
            title: "Tray GUI".to_string(),
            status: Some(HandoffStatus::Active),
            created: Some(Timestamp::new("2026-01-01")),
            completed: None,
            pending_work_identifier_raw: Some("GLP-0001".to_string()),
            goals_completed: 0,
            goals_total: 1,
            body: "\n# Tray GUI\n".to_string(),
            source: "---\nstatus: active\nproject: glep-shimeji\ncreated: 2026-01-01\npw: GLP-0001\n---\n\n# Tray GUI\n".to_string(),
            locator: PathBuf::from("/repo/docs/handoffs/2026-01-01-tray-gui.md"),
            modified_timestamp: SystemTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn cancel_rejects_blank_report_during_execution() {
        let command = CancelPendingWork::new(
            "GLP-0001".to_string(),
            Some("2026-07-14".to_string()),
            " \t\n".to_string(),
            Vec::new(),
            false,
        );

        let error = super::execute(&command, &staged(), &registry(), &FixedClock).unwrap_err();

        assert!(matches!(error, CancelPendingWorkError::EmptyReport));
        assert_eq!(error.to_string(), "--report cannot be empty.");
    }

    #[test]
    fn cancel_handoff_preflight_precedes_blank_report_validation() {
        let tagged = PendingWorkItem {
            tags: Some("[handoff]".to_string()),
            ..record("GLP-0001")
        };
        let store = InMemoryStore::default()
            .with_prefix("glep-shimeji", "GLP")
            .with_project("glep-shimeji", vec![tagged]);
        let command = CancelPendingWork::new(
            "GLP-0001".to_string(),
            Some("2026-07-14".to_string()),
            " \t\n".to_string(),
            Vec::new(),
            false,
        );

        let error = super::execute(&command, &store, &registry(), &FixedClock).unwrap_err();

        assert!(matches!(error, CancelPendingWorkError::HandoffPreflight(_)));
        assert_eq!(
            store.items("glep-shimeji")[0].status,
            WorkItemStatus::Active
        );
    }

    #[test]
    fn cancel_marks_item_cancelled() {
        let store = staged();
        let command = CancelPendingWork::new(
            "GLP-0001".to_string(),
            Some("2026-07-14".to_string()),
            "obsoleted".to_string(),
            vec![" a..b, c..d ".to_string(), "a..b".to_string()],
            false,
        );

        let out = super::execute(&command, &store, &registry(), &FixedClock).unwrap();

        assert_eq!(out.action, super::super::done::ClosedItemAction::Cancelled);
        assert_eq!(
            store.items("glep-shimeji")[0].status,
            WorkItemStatus::Cancelled
        );
        assert_eq!(
            store.items("glep-shimeji")[0].completed,
            Some(Timestamp::new("2026-07-14"))
        );
        assert_eq!(
            store.items("glep-shimeji")[0].commits.as_deref(),
            Some("a..b, c..d")
        );
    }

    #[test]
    fn cancel_uses_clock_date_when_no_date_is_explicit() {
        let store = staged();
        let command = CancelPendingWork::new(
            "GLP-0001".to_string(),
            None,
            "obsoleted".to_string(),
            Vec::new(),
            false,
        );

        super::execute(&command, &store, &registry(), &FixedClock).unwrap();

        assert_eq!(
            store.items("glep-shimeji")[0].completed,
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

        let error = super::execute(&command, &staged(), &registry(), &FixedClock).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Unknown task id prefix `XYZ` for XYZ-0001"
        );
    }

    #[test]
    fn cancel_reports_handoff_failure_after_pending_work_is_cancelled() {
        let tagged = PendingWorkItem {
            tags: Some("[handoff]".to_string()),
            ..record("GLP-0001")
        };
        let scope = HandoffScope {
            repository_root: PathBuf::from("/repo"),
        };
        let store = InMemoryStore::default()
            .with_prefix("glep-shimeji", "GLP")
            .with_project("glep-shimeji", vec![tagged])
            .with_handoff_documents(scope, vec![handoff()])
            .with_failure(FailurePoint::DocumentUpdate);
        let command = CancelPendingWork::new(
            "GLP-0001".to_string(),
            Some("2026-07-14".to_string()),
            "obsoleted".to_string(),
            Vec::new(),
            false,
        );

        let error = super::execute(&command, &store, &registry(), &FixedClock).unwrap_err();

        assert!(matches!(
            error,
            CancelPendingWorkError::HandoffAfterPendingWork {
                ref pending_work_identifier,
                ..
            } if pending_work_identifier.as_ref() == "GLP-0001"
        ));
        assert_eq!(
            store.items("glep-shimeji")[0].status,
            WorkItemStatus::Cancelled
        );
    }
}
