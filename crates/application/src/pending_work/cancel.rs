use pwf_domain::pending_work::ProjectRegistry;

use super::{
    add::AddPendingWorkError,
    done::{CloseError, ClosedItemAction, CompletedPendingWork, perform_close},
};
use crate::ports::{AppDbStore, IndexEntry, IndexSection, PendingWorkItem};

#[derive(Debug, Clone)]
pub struct CancelPendingWork {
    pub id: String,
    pub completed: String,
    report: String,
    pub commits: Vec<String>,
    pub review: bool,
}

impl CancelPendingWork {
    pub fn new(
        id: String,
        completed: String,
        report: String,
        commits: Vec<String>,
        review: bool,
    ) -> Result<Self, CancelPendingWorkError> {
        if report.trim().is_empty() {
            return Err(CancelPendingWorkError::EmptyReport);
        }
        Ok(Self {
            id,
            completed,
            report,
            commits,
            review,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CancelPendingWorkError {
    #[error("--report cannot be empty.")]
    EmptyReport,
    /// Verbatim former infra `ItemNotFound` display (PWF-0123 error-string
    /// relocation) — also covers an already-closed item.
    #[error("Open pending-work item not found: {id}")]
    ItemNotFound { id: String },
    #[error("{0}")]
    WriteStore(Box<dyn std::error::Error + Send + Sync>),
    #[error("{0}")]
    ReviewTask(#[source] AddPendingWorkError),
}

#[cqrsy::handler(command)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "the cqrsy cancel operation owns its request by contract"
)]
pub fn execute<S>(
    command: CancelPendingWork,
    store: &S,
    projects: &ProjectRegistry,
) -> Result<CompletedPendingWork, CancelPendingWorkError>
where
    S: AppDbStore<PendingWorkItem> + AppDbStore<IndexEntry> + AppDbStore<IndexSection>,
{
    perform_close(
        store,
        projects,
        ClosedItemAction::Cancelled,
        &command.id,
        &command.completed,
        Some(command.report.as_str()),
        &command.commits,
        command.review,
    )
    .map_err(map_close_error)
}

fn map_close_error(error: CloseError) -> CancelPendingWorkError {
    match error {
        CloseError::ItemNotFound { id } => CancelPendingWorkError::ItemNotFound { id },
        CloseError::EmptyReport => CancelPendingWorkError::EmptyReport,
        CloseError::WriteStore(source) => CancelPendingWorkError::WriteStore(source),
        CloseError::ReviewTask(source) => CancelPendingWorkError::ReviewTask(source),
    }
}

#[cfg(test)]
mod tests {
    use pwf_domain::pending_work::{
        ProjectName, ProjectRegistry, Timestamp, WorkItemId, WorkItemStatus,
    };

    use super::{CancelPendingWork, CancelPendingWorkError, execute};
    use crate::{
        IndexEntry, IndexEntryState, Materialization, PendingWorkItem, RecordId,
        testing::InMemoryStore,
    };

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
        <InMemoryStore as crate::AppDbStore<IndexEntry>>::insert(
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

    #[test]
    fn cancel_requires_report() {
        let error = CancelPendingWork::new(
            "GLP-0001".to_string(),
            "2026-07-14".to_string(),
            " \t\n".to_string(),
            Vec::new(),
            false,
        )
        .expect_err("blank cancellation report must fail");

        assert!(matches!(error, CancelPendingWorkError::EmptyReport));
        assert_eq!(error.to_string(), "--report cannot be empty.");
    }

    #[test]
    fn cancel_marks_item_cancelled() {
        let store = staged();
        let command = CancelPendingWork::new(
            "GLP-0001".to_string(),
            "2026-07-14".to_string(),
            "obsoleted".to_string(),
            Vec::new(),
            false,
        )
        .unwrap();

        let out = execute(command, &store, &registry()).unwrap();

        assert_eq!(out.action, super::super::done::ClosedItemAction::Cancelled);
        assert_eq!(
            store.items("glep-shimeji")[0].status,
            WorkItemStatus::Cancelled
        );
    }
}
