use std::path::PathBuf;

use super::{
    ProjectRegistry,
    find_pending_work::FindPendingWorkError,
    identifier,
    logic::finding::find_open_item,
    store_util::{self, LoadItemError},
};
use crate::ports::{
    app_record::AppRecordStore,
    confirmation::{Confirmation, ConfirmationClient},
    pending_work_record::{IndexEntry, Materialization, PendingWorkRecord},
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
}

#[derive(Debug, Clone)]
pub struct RemovePendingWorkItem {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemovePendingWorkItemOk {
    Removed(RemovedItem),
    Aborted { pending_work_identifier: String },
}

#[derive(Debug, thiserror::Error)]
pub enum RemovePendingWorkError {
    #[error("Pending-work item not found: {id}")]
    ItemNotFound { id: String },
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
}

/// Deletes an item after unlinking its index entry.
///
/// An unlink failure leaves the note untouched.
#[cqrsy::command]
pub fn execute(
    cmd: &RemovePendingWorkItem,
    store: &(impl AppRecordStore<PendingWorkRecord> + AppRecordStore<IndexEntry>),
    projects: &ProjectRegistry,
    confirmation_client: &(impl ConfirmationClient + Send + Sync + 'static),
) -> Result<RemovePendingWorkItemOk, RemovePendingWorkError> {
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
    let project_id = pending_work_identifier
        .as_ref()
        .split_once('-')
        .map_or("", |(project_id, _)| project_id);
    let project = projects
        .get_project_name_by(&pending_work_identifier)
        .ok_or_else(|| RemovePendingWorkError::UnknownPrefix {
            pending_work_identifier: pending_work_identifier.to_string(),
            prefix: project_id.to_string(),
        })?;
    let record =
        store_util::require_item(store, project, &pending_work_identifier).map_err(|error| {
            match error {
                LoadItemError::ItemNotFound { id } => RemovePendingWorkError::ItemNotFound { id },
                LoadItemError::Store(source) => RemovePendingWorkError::WriteStore(source),
            }
        })?;
    let note_path = match &record.materialization {
        Materialization::NoteFile => PathBuf::from(&record.locator),
        Materialization::MissingNote { expected } => {
            return Err(RemovePendingWorkError::NoteMissing {
                path: expected.clone(),
            });
        }
        Materialization::InlineLegacy => return Err(RemovePendingWorkError::FileModelRequired),
    };
    let confirmation = Confirmation::Removal {
        pending_work_identifier: pending_work_identifier.clone(),
        project: project.clone(),
        title: record.title.clone(),
        status: record.status,
        note_path: note_path.clone(),
    };
    if !confirmation_client.confirm(&confirmation) {
        return Ok(RemovePendingWorkItemOk::Aborted {
            pending_work_identifier: pending_work_identifier.to_string(),
        });
    }

    AppRecordStore::<IndexEntry>::delete(store, project, &pending_work_identifier)
        .map_err(|error| RemovePendingWorkError::WriteStore(Box::new(error)))?;
    AppRecordStore::<PendingWorkRecord>::delete(store, project, &pending_work_identifier)
        .map_err(|error| RemovePendingWorkError::WriteStore(Box::new(error)))?;

    let removed = RemovedItem {
        id: pending_work_identifier.as_ref().to_string(),
        project: project.as_ref().to_string(),
        title: record.title,
        deleted_path: note_path,
        unlinked: record
            .placement
            .map(|placement| placement.index_path)
            .unwrap_or_default(),
    };
    Ok(RemovePendingWorkItemOk::Removed(removed))
}

#[cfg(test)]
mod tests {
    use pwf_models::pending_work::{ProjectName, Timestamp, WorkItemId, WorkItemStatus};

    use super::{
        ProjectRegistry, RemovePendingWorkError, RemovePendingWorkItem, RemovePendingWorkItemOk,
        execute,
    };
    use crate::{
        ports::{
            app_record::AppRecordStore,
            confirmation::{Confirmation, ConfirmationClient},
            pending_work_record::{
                IndexEntry, IndexEntryState, Materialization, PendingWorkRecord, RecordId,
            },
        },
        testing::InMemoryStore,
    };

    fn registry() -> ProjectRegistry {
        ProjectRegistry::new(vec![(
            ProjectName::try_new("pwf").unwrap(),
            Some("/repo/pwf".to_string()),
            Some("PWF".to_string()),
        )])
    }

    fn record(id: &str, status: WorkItemStatus) -> PendingWorkRecord {
        PendingWorkRecord {
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
        let index_state = match status {
            WorkItemStatus::Active => IndexEntryState::Open,
            WorkItemStatus::Done | WorkItemStatus::Cancelled => {
                IndexEntryState::Done(Timestamp::new("2026-07-02"))
            }
        };
        let store = InMemoryStore::default()
            .with_prefix("pwf", "PWF")
            .with_project("pwf", vec![record("PWF-0001", status)]);
        <InMemoryStore as AppRecordStore<IndexEntry>>::insert(
            &store,
            &ProjectName::try_new("pwf").unwrap(),
            IndexEntry {
                id: WorkItemId::try_new("PWF-0001").unwrap(),
                state: index_state,
                section: String::new(),
            },
        )
        .unwrap();
        store
    }

    fn command(id: &str) -> RemovePendingWorkItem {
        RemovePendingWorkItem { id: id.to_string() }
    }

    #[derive(Clone)]
    struct Accepted;

    impl ConfirmationClient for Accepted {
        fn confirm(&self, _confirmation: &Confirmation) -> bool {
            true
        }
    }

    #[test]
    fn remove_deletes_record_and_index_entry() {
        let store = staged(WorkItemStatus::Active);

        let RemovePendingWorkItemOk::Removed(removed) =
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
    fn remove_deletes_closed_items() {
        for status in [WorkItemStatus::Done, WorkItemStatus::Cancelled] {
            let store = staged(status);

            let outcome =
                super::execute(&command("PWF-0001"), &store, &registry(), &Accepted).unwrap();

            assert!(matches!(outcome, RemovePendingWorkItemOk::Removed(_)));
            assert!(store.items("pwf").is_empty(), "{status} record retained");
            assert!(store.entries("pwf").is_empty(), "{status} index retained");
        }
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
    fn remove_rejects_missing_note_wikilink_with_legacy_display() {
        let ghost = PendingWorkRecord {
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
        use super::{super::RemovePendingWorkItemOk, *};

        fn command(id: &str) -> RemovePendingWorkItem {
            RemovePendingWorkItem { id: id.to_string() }
        }

        #[derive(Clone)]
        struct StaticInteraction {
            accepted: bool,
        }

        impl ConfirmationClient for StaticInteraction {
            fn confirm(&self, _confirmation: &Confirmation) -> bool {
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
            let RemovePendingWorkItemOk::Removed(removed) = outcome else {
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
                RemovePendingWorkItemOk::Aborted {
                    pending_work_identifier: "PWF-0001".to_string(),
                }
            );
            assert_eq!(store.items("pwf").len(), 1);
            assert_eq!(store.entries("pwf").len(), 1);
        }
    }
}
