use pwf_domain::pending_work::{
    ProjectName, QueueEntryView, ReopenDecision, WorkItemId, WorkItemStatus, reopen_decision,
};

use super::{
    done::queue_view,
    project_registry::ProjectRegistry,
    store_util::{self, LoadItemError},
};
use crate::{
    HandoffDocumentStore, HandoffLedger,
    handoff::{HandoffError, HandoffMutationOk, lifecycle},
    ports::{AppRecordStore, IndexEntry, IndexEntryState, ItemPatch, PendingWorkItem},
};

#[derive(Debug, Clone)]
pub struct ReopenPendingWork {
    pub id: String,
}

/// Contains the outcome of reopening a closed item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReopenedPendingWork {
    pub id: WorkItemId,
    pub project: ProjectName,
    /// Indicates an idempotent skip with no mutation.
    pub already_active: bool,
    pub handoff: HandoffMutationOk,
}

#[derive(Debug, thiserror::Error)]
pub enum ReopenPendingWorkError {
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
    #[error(transparent)]
    HandoffPreflight(HandoffError),
    #[error("{source}")]
    HandoffAfterPendingWork {
        pending_work_identifier: WorkItemId,
        #[source]
        source: HandoffError,
    },
}

/// Reopens a closed item and restores or re-adds its queue link.
///
/// The update clears `completed:` and `commits:`. An active item returns an idempotent skip.
#[cqrsy::command]
pub fn execute<S>(
    cmd: &ReopenPendingWork,
    store: &S,
    projects: &ProjectRegistry,
) -> Result<ReopenedPendingWork, ReopenPendingWorkError>
where
    S: AppRecordStore<PendingWorkItem>
        + AppRecordStore<IndexEntry>
        + HandoffDocumentStore
        + AppRecordStore<HandoffLedger>,
{
    let not_found = || ReopenPendingWorkError::ItemNotFound { id: cmd.id.clone() };
    let pending_work_identifier = WorkItemId::try_new(&cmd.id).map_err(|_| not_found())?;
    let prefix = pending_work_identifier
        .as_ref()
        .split_once('-')
        .map_or("", |(prefix, _)| prefix);
    let project = projects
        .project_for_id(&pending_work_identifier)
        .ok_or_else(|| ReopenPendingWorkError::UnknownPrefix {
            pending_work_identifier: pending_work_identifier.to_string(),
            prefix: prefix.to_string(),
        })?;
    let record =
        store_util::require_item(store, project, &pending_work_identifier).map_err(|error| {
            match error {
                LoadItemError::ItemNotFound { id } => ReopenPendingWorkError::ItemNotFound { id },
                LoadItemError::Store(source) => ReopenPendingWorkError::WriteStore(source),
            }
        })?;

    let handoff_pending =
        lifecycle::preflight_reopen(store, projects, pending_work_identifier.as_ref())
            .map_err(ReopenPendingWorkError::HandoffPreflight)?;

    if record.status == WorkItemStatus::Active {
        return Ok(ReopenedPendingWork {
            id: pending_work_identifier.clone(),
            project: project.clone(),
            already_active: true,
            handoff: lifecycle::commit_after_pending_work(store, handoff_pending).map_err(
                |source| ReopenPendingWorkError::HandoffAfterPendingWork {
                    pending_work_identifier: pending_work_identifier.clone(),
                    source,
                },
            )?,
        });
    }

    <S as AppRecordStore<PendingWorkItem>>::update(
        store,
        project,
        &pending_work_identifier,
        ItemPatch {
            status: Some(WorkItemStatus::Active),
            completed: Some(None),
            commits: Some(None),
            ..ItemPatch::default()
        },
    )
    .map_err(|error| ReopenPendingWorkError::WriteStore(Box::new(error)))?;

    let entries = <S as AppRecordStore<IndexEntry>>::list(store, project)
        .map_err(|error| ReopenPendingWorkError::WriteStore(Box::new(error)))?;
    let views: Vec<QueueEntryView> = entries.iter().map(queue_view).collect();
    let open_entry = IndexEntry {
        id: pending_work_identifier.clone(),
        state: IndexEntryState::Open,
        section: String::new(),
    };
    match reopen_decision(&views, &pending_work_identifier) {
        ReopenDecision::RestoreExisting => {
            <S as AppRecordStore<IndexEntry>>::update(
                store,
                project,
                &pending_work_identifier,
                open_entry,
            )
            .map_err(|error| ReopenPendingWorkError::WriteStore(Box::new(error)))?;
        }
        ReopenDecision::ReAddEvicted => {
            <S as AppRecordStore<IndexEntry>>::insert(store, project, open_entry)
                .map_err(|error| ReopenPendingWorkError::WriteStore(Box::new(error)))?;
        }
        ReopenDecision::AlreadyOpen => {}
    }

    let handoff =
        lifecycle::commit_after_pending_work(store, handoff_pending).map_err(|source| {
            ReopenPendingWorkError::HandoffAfterPendingWork {
                pending_work_identifier: pending_work_identifier.clone(),
                source,
            }
        })?;
    Ok(ReopenedPendingWork {
        id: pending_work_identifier,
        project: project.clone(),
        already_active: false,
        handoff,
    })
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::SystemTime};

    use pwf_domain::{
        handoff::HandoffStatus,
        pending_work::{ProjectName, Timestamp, WorkItemId, WorkItemStatus},
    };

    use super::{ProjectRegistry, ReopenPendingWork, ReopenPendingWorkError, execute};
    use crate::{
        HandoffDocument, HandoffDocumentIdentifier, HandoffLocation, HandoffScope, IndexEntry,
        IndexEntryState, Materialization, PendingWorkItem, RecordId,
        handoff::HandoffMutationOk,
        testing::{FailurePoint, InMemoryStore},
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
            <InMemoryStore as crate::AppRecordStore<IndexEntry>>::insert(&store, &glp(), entry)
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

        let out = execute(&command(), &store, &registry()).unwrap();

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

        let out = execute(&command(), &store, &registry()).unwrap();

        assert!(!out.already_active);
        let entries = store.entries("glep-shimeji");
        assert_eq!(entries.len(), 1, "evicted link must be re-added");
        assert_eq!(entries[0].id, WorkItemId::try_new("GLP-0001").unwrap());
        assert_eq!(entries[0].state, IndexEntryState::Open);
    }

    #[test]
    fn reopen_already_active_is_idempotent_skip() {
        let store = staged(WorkItemStatus::Active, vec![entry(IndexEntryState::Open)]);

        let out = execute(&command(), &store, &registry()).unwrap();

        assert!(out.already_active);
        assert_eq!(
            store.items("glep-shimeji")[0].commits.as_deref(),
            Some("a..b")
        );
    }

    #[test]
    fn reopen_reports_an_unknown_configured_prefix() {
        let store = staged(WorkItemStatus::Done, Vec::new());
        let command = ReopenPendingWork {
            id: "XYZ-0001".to_string(),
        };

        let error = execute(&command, &store, &registry()).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Unknown task id prefix `XYZ` for XYZ-0001"
        );
    }

    #[test]
    fn reopen_restores_the_linked_archived_handoff() {
        let tagged = PendingWorkItem {
            tags: Some("[handoff]".to_string()),
            ..record("GLP-0001", WorkItemStatus::Done)
        };
        let scope = HandoffScope {
            repository_root: PathBuf::from("/repo"),
        };
        let handoff = HandoffDocument {
            identifier: HandoffDocumentIdentifier {
                file_name: "2026-01-01-tray-gui.md".to_string(),
                location: HandoffLocation::Archived,
            },
            location: HandoffLocation::Archived,
            project: Some(glp()),
            title: "Tray GUI".to_string(),
            status: Some(HandoffStatus::Done),
            created: Some(Timestamp::new("2026-01-01")),
            completed: Some(Timestamp::new("2026-01-02")),
            pending_work_identifier_raw: Some("GLP-0001".to_string()),
            goals_completed: 1,
            goals_total: 1,
            body: "\n# Tray GUI\n".to_string(),
            source: "---\nstatus: done\ncompleted: 2026-01-02\nproject: glep-shimeji\ncreated: 2026-01-01\npw: GLP-0001\n---\n\n# Tray GUI\n".to_string(),
            locator: PathBuf::from(
                "/repo/docs/handoffs/archived/2026-01-01-tray-gui.md",
            ),
            modified_timestamp: SystemTime::UNIX_EPOCH,
        };
        let store = InMemoryStore::default()
            .with_prefix("glep-shimeji", "GLP")
            .with_project("glep-shimeji", vec![tagged])
            .with_handoff_documents(scope.clone(), vec![handoff]);

        let outcome = execute(&command(), &store, &registry()).unwrap();

        assert!(matches!(
            outcome.handoff,
            HandoffMutationOk::Reopened { .. }
        ));
        assert_eq!(
            store.handoff_documents(&scope)[0].location,
            HandoffLocation::Active
        );
    }

    #[test]
    fn reopen_reports_handoff_failure_after_pending_work_is_reopened() {
        let tagged = PendingWorkItem {
            tags: Some("[handoff]".to_string()),
            ..record("GLP-0001", WorkItemStatus::Done)
        };
        let scope = HandoffScope {
            repository_root: PathBuf::from("/repo"),
        };
        let handoff = HandoffDocument {
            identifier: HandoffDocumentIdentifier {
                file_name: "2026-01-01-tray-gui.md".to_string(),
                location: HandoffLocation::Archived,
            },
            location: HandoffLocation::Archived,
            project: Some(glp()),
            title: "Tray GUI".to_string(),
            status: Some(HandoffStatus::Done),
            created: Some(Timestamp::new("2026-01-01")),
            completed: Some(Timestamp::new("2026-01-02")),
            pending_work_identifier_raw: Some("GLP-0001".to_string()),
            goals_completed: 1,
            goals_total: 1,
            body: "\n# Tray GUI\n".to_string(),
            source: "---\nstatus: done\ncompleted: 2026-01-02\nproject: glep-shimeji\ncreated: 2026-01-01\npw: GLP-0001\n---\n\n# Tray GUI\n".to_string(),
            locator: PathBuf::from(
                "/repo/docs/handoffs/archived/2026-01-01-tray-gui.md",
            ),
            modified_timestamp: SystemTime::UNIX_EPOCH,
        };
        let store = InMemoryStore::default()
            .with_prefix("glep-shimeji", "GLP")
            .with_project("glep-shimeji", vec![tagged])
            .with_handoff_documents(scope, vec![handoff])
            .with_failure(FailurePoint::DocumentUpdate);

        let error = execute(&command(), &store, &registry()).unwrap_err();

        assert!(matches!(
            error,
            ReopenPendingWorkError::HandoffAfterPendingWork {
                ref pending_work_identifier,
                ..
            } if pending_work_identifier.as_ref() == "GLP-0001"
        ));
        assert_eq!(
            store.items("glep-shimeji")[0].status,
            WorkItemStatus::Active
        );
    }
}
