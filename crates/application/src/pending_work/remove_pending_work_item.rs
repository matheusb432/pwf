use std::path::{Path, PathBuf};

use pwf_domain::pending_work::{ProjectName, WorkItemId, WorkItemStatus};

use super::{
    find_pending_work::{FindPendingWorkError, find_open_item},
    identifier,
    project_registry::ProjectRegistry,
    store_util::{self, LoadItemError},
};
use crate::{
    HandoffDocumentStore, HandoffLedger,
    handoff::{HandoffError, HandoffMutationOk, lifecycle},
    ports::{AppRecordStore, IndexEntry, Materialization, PendingWorkItem},
};

/// Describes the note and index link deleted by [`execute`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovedItem {
    /// Canonical identifier of the deleted item.
    pub id: String,
    /// Managed project that contained the item.
    pub project: String,
    /// Title of the deleted item.
    pub title: String,
    /// Path of the deleted item note.
    pub deleted_path: PathBuf,
    /// Index note from which the item link was removed.
    pub unlinked: String,
    pub handoff: HandoffMutationOk,
}

#[derive(Debug, Clone)]
pub struct RemovePendingWorkItem {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovalConfirmation {
    pub pending_work_identifier: WorkItemId,
    pub project: ProjectName,
    pub title: String,
    pub note_path: PathBuf,
    pub handoff_path: Option<PathBuf>,
}

pub trait RemovalInteraction: Clone + Send + Sync + 'static {
    fn confirm(&self, confirmation: &RemovalConfirmation) -> bool;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemovePendingWorkOutcome {
    Removed(RemovedItem),
    Aborted { pending_work_identifier: String },
}

#[derive(Debug, thiserror::Error)]
pub enum RemovePendingWorkError {
    #[error("Open pending-work item not found: {id}")]
    ItemNotFound { id: String },
    /// Reports a canonical identifier whose prefix has no configured project.
    #[error("Unknown task id prefix `{prefix}` for {pending_work_identifier}")]
    UnknownPrefix {
        pending_work_identifier: String,
        prefix: String,
    },
    #[error("Work-item note missing: {path}")]
    NoteMissing { path: String },
    #[error("remove only supports file-model pending-work items.")]
    FileModelRequired,
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

/// Deletes an open item after unlinking its index entry.
///
/// An unlink failure leaves the note untouched.
#[cqrsy::command]
pub fn execute<S, I>(
    cmd: &RemovePendingWorkItem,
    store: &S,
    projects: &ProjectRegistry,
    interaction: &I,
) -> Result<RemovePendingWorkOutcome, RemovePendingWorkError>
where
    S: AppRecordStore<PendingWorkItem>
        + AppRecordStore<IndexEntry>
        + HandoffDocumentStore
        + AppRecordStore<HandoffLedger>,
    I: RemovalInteraction,
{
    let not_found = || RemovePendingWorkError::ItemNotFound { id: cmd.id.clone() };
    let Some(pending_work_identifier) = identifier::parse(&cmd.id) else {
        return match find_open_item(store, projects, &cmd.id) {
            Ok(_) => Err(RemovePendingWorkError::FileModelRequired),
            Err(FindPendingWorkError::ReadStore(source)) => {
                Err(RemovePendingWorkError::WriteStore(source))
            }
            Err(_) => Err(not_found()),
        };
    };
    let prefix = pending_work_identifier
        .as_ref()
        .split_once('-')
        .map_or("", |(prefix, _)| prefix);
    let project = projects
        .project_for_id(&pending_work_identifier)
        .ok_or_else(|| RemovePendingWorkError::UnknownPrefix {
            pending_work_identifier: pending_work_identifier.to_string(),
            prefix: prefix.to_string(),
        })?;
    let record =
        store_util::require_item(store, project, &pending_work_identifier).map_err(|error| {
            match error {
                LoadItemError::ItemNotFound { id } => RemovePendingWorkError::ItemNotFound { id },
                LoadItemError::Store(source) => RemovePendingWorkError::WriteStore(source),
            }
        })?;
    if record.status != WorkItemStatus::Active {
        return Err(not_found());
    }
    let note_path = match &record.materialization {
        Materialization::NoteFile => PathBuf::from(&record.locator),
        Materialization::MissingNote { expected } => {
            return Err(RemovePendingWorkError::NoteMissing {
                path: expected.clone(),
            });
        }
        Materialization::InlineLegacy => return Err(RemovePendingWorkError::FileModelRequired),
    };
    let handoff_pending =
        lifecycle::preflight_delete(store, projects, pending_work_identifier.as_ref())
            .map_err(RemovePendingWorkError::HandoffPreflight)?;
    let confirmation = RemovalConfirmation {
        pending_work_identifier: pending_work_identifier.clone(),
        project: project.clone(),
        title: record.title.clone(),
        note_path: note_path.clone(),
        handoff_path: lifecycle::pending_handoff_path(&handoff_pending).map(Path::to_path_buf),
    };
    if !interaction.confirm(&confirmation) {
        return Ok(RemovePendingWorkOutcome::Aborted {
            pending_work_identifier: pending_work_identifier.to_string(),
        });
    }

    <S as AppRecordStore<IndexEntry>>::delete(store, project, &pending_work_identifier)
        .map_err(|error| RemovePendingWorkError::WriteStore(Box::new(error)))?;
    <S as AppRecordStore<PendingWorkItem>>::delete(store, project, &pending_work_identifier)
        .map_err(|error| RemovePendingWorkError::WriteStore(Box::new(error)))?;

    let mut removed = RemovedItem {
        id: pending_work_identifier.as_ref().to_string(),
        project: project.as_ref().to_string(),
        title: record.title,
        deleted_path: note_path,
        unlinked: record
            .placement
            .map(|placement| placement.index_path)
            .unwrap_or_default(),
        handoff: HandoffMutationOk::NotLinked,
    };
    removed.handoff =
        lifecycle::commit_after_pending_work(store, handoff_pending).map_err(|source| {
            RemovePendingWorkError::HandoffAfterPendingWork {
                pending_work_identifier,
                source,
            }
        })?;
    Ok(RemovePendingWorkOutcome::Removed(removed))
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::SystemTime};

    use pwf_domain::{
        handoff::HandoffStatus,
        pending_work::{ProjectName, Timestamp, WorkItemId, WorkItemStatus},
    };

    use super::{
        ProjectRegistry, RemovalConfirmation, RemovalInteraction, RemovePendingWorkError,
        RemovePendingWorkItem, RemovePendingWorkOutcome, execute,
    };
    use crate::{
        HandoffDocument, HandoffDocumentIdentifier, HandoffLocation, HandoffScope, IndexEntry,
        IndexEntryState, Materialization, PendingWorkItem, RecordId,
        testing::{FailurePoint, InMemoryStore},
    };

    fn registry() -> ProjectRegistry {
        ProjectRegistry::new(vec![(
            ProjectName::try_new("pwf").unwrap(),
            Some("/repo/pwf".to_string()),
            Some("PWF".to_string()),
        )])
    }

    fn record(id: &str, status: WorkItemStatus) -> PendingWorkItem {
        PendingWorkItem {
            id: RecordId::Item(WorkItemId::try_new(id).unwrap()),
            title: "stale task".to_string(),
            status,
            created: Some(Timestamp::new("2026-07-01")),
            completed: None,
            commits: None,
            tags: None,
            effort: None,
            prereq: None,
            section: None,
            body: "body".to_string(),
            source: "body".to_string(),
            locator: format!("/notes/pwf/{id}.md"),
            placement: None,
            materialization: Materialization::NoteFile,
        }
    }

    fn staged(status: WorkItemStatus) -> InMemoryStore {
        let store = InMemoryStore::default()
            .with_prefix("pwf", "PWF")
            .with_project("pwf", vec![record("PWF-0001", status)]);
        <InMemoryStore as crate::AppRecordStore<IndexEntry>>::insert(
            &store,
            &ProjectName::try_new("pwf").unwrap(),
            IndexEntry {
                id: WorkItemId::try_new("PWF-0001").unwrap(),
                state: IndexEntryState::Open,
                section: String::new(),
            },
        )
        .unwrap();
        store
    }

    fn command(id: &str) -> RemovePendingWorkItem {
        RemovePendingWorkItem { id: id.to_string() }
    }

    fn handoff() -> HandoffDocument {
        HandoffDocument {
            identifier: HandoffDocumentIdentifier {
                file_name: "2026-07-01-stale-task.md".to_string(),
                location: HandoffLocation::Active,
            },
            location: HandoffLocation::Active,
            project: Some(ProjectName::try_new("pwf").unwrap()),
            title: "Stale task".to_string(),
            status: Some(HandoffStatus::Active),
            created: Some(Timestamp::new("2026-07-01")),
            completed: None,
            pending_work_identifier_raw: Some("PWF-0001".to_string()),
            goals_completed: 0,
            goals_total: 1,
            body: "\n# Stale task\n".to_string(),
            source: "---\nstatus: active\nproject: pwf\ncreated: 2026-07-01\npw: PWF-0001\n---\n\n# Stale task\n".to_string(),
            locator: PathBuf::from("/repo/pwf/docs/handoffs/2026-07-01-stale-task.md"),
            modified_timestamp: SystemTime::UNIX_EPOCH,
        }
    }

    #[derive(Clone)]
    struct Accepted;

    impl RemovalInteraction for Accepted {
        fn confirm(&self, _confirmation: &RemovalConfirmation) -> bool {
            true
        }
    }

    #[test]
    fn remove_deletes_record_and_index_entry() {
        let store = staged(WorkItemStatus::Active);

        let RemovePendingWorkOutcome::Removed(removed) =
            super::execute(&command("PWF-0001"), &store, &registry(), &Accepted).unwrap()
        else {
            panic!("accepted removal must remove the item");
        };

        assert_eq!(removed.id, "PWF-0001");
        assert_eq!(removed.project, "pwf");
        assert_eq!(removed.title, "stale task");
        assert!(store.items("pwf").is_empty(), "record must be deleted");
        assert!(
            store.entries("pwf").is_empty(),
            "index entry must be unlinked"
        );
    }

    #[test]
    fn remove_rejects_closed_item_with_open_not_found_display() {
        let store = staged(WorkItemStatus::Done);

        let error =
            super::execute(&command("PWF-0001"), &store, &registry(), &Accepted).unwrap_err();

        assert!(matches!(
            error,
            RemovePendingWorkError::ItemNotFound { ref id } if id == "PWF-0001"
        ));
        assert_eq!(
            error.to_string(),
            "Open pending-work item not found: PWF-0001"
        );
        assert_eq!(store.items("pwf").len(), 1, "nothing may be deleted");
    }

    #[test]
    fn remove_missing_item_preserves_requested_id() {
        let store = staged(WorkItemStatus::Active);

        let error =
            super::execute(&command("PWF-9999"), &store, &registry(), &Accepted).unwrap_err();

        assert!(matches!(
            error,
            RemovePendingWorkError::ItemNotFound { ref id } if id == "PWF-9999"
        ));
    }

    #[test]
    fn remove_reports_an_unknown_configured_prefix() {
        let store = staged(WorkItemStatus::Active);

        let error =
            super::execute(&command("XYZ-0001"), &store, &registry(), &Accepted).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Unknown task id prefix `XYZ` for XYZ-0001"
        );
    }

    #[test]
    fn remove_reports_handoff_failure_after_pending_work_is_deleted() {
        let tagged = PendingWorkItem {
            tags: Some("[handoff]".to_string()),
            ..record("PWF-0001", WorkItemStatus::Active)
        };
        let scope = HandoffScope {
            repository_root: PathBuf::from("/repo/pwf"),
        };
        let store = staged(WorkItemStatus::Active)
            .with_project("pwf", vec![tagged])
            .with_handoff_documents(scope, vec![handoff()])
            .with_failure(FailurePoint::DocumentDelete);

        let error =
            super::execute(&command("PWF-0001"), &store, &registry(), &Accepted).unwrap_err();

        assert!(matches!(
            error,
            RemovePendingWorkError::HandoffAfterPendingWork {
                ref pending_work_identifier,
                ..
            } if pending_work_identifier.as_ref() == "PWF-0001"
        ));
        assert!(store.items("pwf").is_empty());
    }

    #[test]
    fn remove_rejects_missing_note_wikilink_with_legacy_display() {
        let ghost = PendingWorkItem {
            materialization: Materialization::MissingNote {
                expected: "/notes/pwf/PWF-0001.md".to_string(),
            },
            ..record("PWF-0001", WorkItemStatus::Active)
        };
        let store = InMemoryStore::default()
            .with_prefix("pwf", "PWF")
            .with_project("pwf", vec![ghost]);

        let error =
            super::execute(&command("PWF-0001"), &store, &registry(), &Accepted).unwrap_err();

        assert!(matches!(
            error,
            RemovePendingWorkError::NoteMissing { ref path } if path == "/notes/pwf/PWF-0001.md"
        ));
        assert_eq!(
            error.to_string(),
            "Work-item note missing: /notes/pwf/PWF-0001.md"
        );
    }

    mod pwf_0144 {
        use super::{
            super::{RemovalConfirmation, RemovalInteraction, RemovePendingWorkOutcome},
            *,
        };

        fn command(id: &str) -> RemovePendingWorkItem {
            RemovePendingWorkItem { id: id.to_string() }
        }

        #[derive(Clone)]
        struct StaticInteraction {
            accepted: bool,
        }

        impl RemovalInteraction for StaticInteraction {
            fn confirm(&self, _confirmation: &RemovalConfirmation) -> bool {
                self.accepted
            }
        }

        #[test]
        fn remove_deletes_record_and_index_entry_after_confirmation() {
            let store = staged(WorkItemStatus::Active);

            let outcome = super::execute(
                &command("PWF-0001"),
                &store,
                &registry(),
                &StaticInteraction { accepted: true },
            )
            .unwrap();
            let RemovePendingWorkOutcome::Removed(removed) = outcome else {
                panic!("accepted removal must remove the item");
            };

            assert_eq!(removed.id, "PWF-0001");
            assert!(store.items("pwf").is_empty());
            assert!(store.entries("pwf").is_empty());
        }

        #[test]
        fn remove_decline_returns_aborted_without_mutating_pending_work() {
            let store = staged(WorkItemStatus::Active);

            let outcome = super::execute(
                &command("PWF-0001"),
                &store,
                &registry(),
                &StaticInteraction { accepted: false },
            )
            .unwrap();

            assert_eq!(
                outcome,
                RemovePendingWorkOutcome::Aborted {
                    pending_work_identifier: "PWF-0001".to_string(),
                }
            );
            assert_eq!(store.items("pwf").len(), 1);
            assert_eq!(store.entries("pwf").len(), 1);
        }
    }
}
