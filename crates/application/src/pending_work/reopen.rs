use pwf_domain::pending_work::{
    ProjectName, ProjectRegistry, QueueEntryView, ReopenDecision, WorkItemId, WorkItemStatus,
    reopen_decision,
};

use super::{
    done::queue_view,
    store_util::{self, LoadItemError},
};
use crate::ports::{AppDbStore, IndexEntry, IndexEntryState, ItemPatch, PendingWorkItem};

#[derive(Debug, Clone)]
pub struct ReopenPendingWork {
    pub id: String,
}

/// The result of reopening a done/cancelled item back to active.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReopenedPendingWork {
    pub id: WorkItemId,
    pub project: ProjectName,
    /// The item was already active — an idempotent skip that mutated nothing.
    pub already_active: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ReopenPendingWorkError {
    /// Verbatim former infra `ItemNotFound` display (PWF-0123 error-string
    /// relocation).
    #[error("Open pending-work item not found: {id}")]
    ItemNotFound { id: String },
    #[error("{0}")]
    WriteStore(Box<dyn std::error::Error + Send + Sync>),
}

/// Flips a done/cancelled item back to active: clears the `completed:`/`commits:`
/// provenance in one [`ItemPatch`], then restores or re-adds its open done-queue
/// link per the pure [`reopen_decision`]. Idempotent — an already-active item is
/// reported as a skip and mutates nothing.
#[cqrsy::handler(command)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "the cqrsy reopen operation owns its request by contract"
)]
pub fn execute<S>(
    cmd: ReopenPendingWork,
    store: &S,
    projects: &ProjectRegistry,
) -> Result<ReopenedPendingWork, ReopenPendingWorkError>
where
    S: AppDbStore<PendingWorkItem> + AppDbStore<IndexEntry>,
{
    let not_found = || ReopenPendingWorkError::ItemNotFound { id: cmd.id.clone() };
    let id = WorkItemId::try_new(&cmd.id).map_err(|_| not_found())?;
    let project = projects.project_for_id(&id).ok_or_else(not_found)?;
    let record = store_util::require_item(store, project, &id).map_err(|error| match error {
        LoadItemError::ItemNotFound { id } => ReopenPendingWorkError::ItemNotFound { id },
        LoadItemError::Store(source) => ReopenPendingWorkError::WriteStore(source),
    })?;

    if record.status == WorkItemStatus::Active {
        return Ok(ReopenedPendingWork {
            id,
            project: project.clone(),
            already_active: true,
        });
    }

    <S as AppDbStore<PendingWorkItem>>::update(
        store,
        project,
        &id,
        ItemPatch {
            status: Some(WorkItemStatus::Active),
            completed: Some(None),
            commits: Some(None),
            ..ItemPatch::default()
        },
    )
    .map_err(|error| ReopenPendingWorkError::WriteStore(Box::new(error)))?;

    let entries = <S as AppDbStore<IndexEntry>>::list(store, project)
        .map_err(|error| ReopenPendingWorkError::WriteStore(Box::new(error)))?;
    let views: Vec<QueueEntryView> = entries.iter().map(queue_view).collect();
    let open_entry = IndexEntry {
        id: id.clone(),
        state: IndexEntryState::Open,
        section: String::new(),
    };
    match reopen_decision(&views, &id) {
        ReopenDecision::RestoreExisting => {
            <S as AppDbStore<IndexEntry>>::update(store, project, &id, open_entry)
                .map_err(|error| ReopenPendingWorkError::WriteStore(Box::new(error)))?;
        }
        ReopenDecision::ReAddEvicted => {
            <S as AppDbStore<IndexEntry>>::insert(store, project, open_entry)
                .map_err(|error| ReopenPendingWorkError::WriteStore(Box::new(error)))?;
        }
        ReopenDecision::AlreadyOpen => {}
    }

    Ok(ReopenedPendingWork {
        id,
        project: project.clone(),
        already_active: false,
    })
}

#[cfg(test)]
mod tests {
    use pwf_domain::pending_work::{
        ProjectName, ProjectRegistry, Timestamp, WorkItemId, WorkItemStatus,
    };

    use super::{ReopenPendingWork, execute};
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

    fn record(id: &str, status: WorkItemStatus) -> PendingWorkItem {
        PendingWorkItem {
            id: RecordId::Item(WorkItemId::try_new(id).unwrap()),
            title: "tray gui".to_string(),
            status,
            created: Some(Timestamp::new("2026-01-01")),
            completed: (status != WorkItemStatus::Active).then(|| Timestamp::new("2026-01-02")),
            commits: Some("a..b".to_string()),
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

    fn glp() -> ProjectName {
        ProjectName::try_new("glep-shimeji").unwrap()
    }

    fn staged(status: WorkItemStatus, entries: Vec<IndexEntry>) -> InMemoryStore {
        let store = InMemoryStore::default()
            .with_prefix("glep-shimeji", "GLP")
            .with_project("glep-shimeji", vec![record("GLP-0001", status)]);
        for entry in entries {
            <InMemoryStore as crate::AppDbStore<IndexEntry>>::insert(&store, &glp(), entry)
                .unwrap();
        }
        store
    }

    fn entry(state: IndexEntryState) -> IndexEntry {
        IndexEntry {
            id: WorkItemId::try_new("GLP-0001").unwrap(),
            state,
            section: String::new(),
        }
    }

    fn command() -> ReopenPendingWork {
        ReopenPendingWork {
            id: "GLP-0001".to_string(),
        }
    }

    #[test]
    fn reopen_restores_done_entry() {
        let store = staged(
            WorkItemStatus::Done,
            vec![entry(IndexEntryState::Done(Timestamp::new("2026-01-02")))],
        );

        let out = execute(command(), &store, &registry()).unwrap();

        assert!(!out.already_active);
        assert_eq!(
            store.items("glep-shimeji")[0].status,
            WorkItemStatus::Active
        );
        assert_eq!(store.items("glep-shimeji")[0].completed, None);
        assert_eq!(store.items("glep-shimeji")[0].commits, None);
        assert_eq!(
            store.entries("glep-shimeji")[0].state,
            IndexEntryState::Open
        );
    }

    #[test]
    fn reopen_re_adds_evicted_entry() {
        let store = staged(WorkItemStatus::Done, Vec::new());

        let out = execute(command(), &store, &registry()).unwrap();

        assert!(!out.already_active);
        let entries = store.entries("glep-shimeji");
        assert_eq!(entries.len(), 1, "evicted link must be re-added");
        assert_eq!(entries[0].id, WorkItemId::try_new("GLP-0001").unwrap());
        assert_eq!(entries[0].state, IndexEntryState::Open);
    }

    #[test]
    fn reopen_already_active_is_idempotent_skip() {
        let store = staged(WorkItemStatus::Active, vec![entry(IndexEntryState::Open)]);

        let out = execute(command(), &store, &registry()).unwrap();

        assert!(out.already_active);
        assert_eq!(
            store.items("glep-shimeji")[0].commits.as_deref(),
            Some("a..b")
        );
    }
}
