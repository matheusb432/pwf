use std::path::PathBuf;

use pwf_domain::pending_work::{ProjectRegistry, RemovedItem, WorkItemId, WorkItemStatus};

use super::store_util::{self, LoadItemError};
use crate::ports::{AppDbStore, IndexEntry, Materialization, PendingWorkItem};

#[derive(Debug, Clone)]
pub struct RemovePendingWorkItem {
    pub id: String,
}

#[derive(Debug, thiserror::Error)]
pub enum RemovePendingWorkError {
    #[error("Open pending-work item not found: {id}")]
    ItemNotFound { id: String },
    #[error("Work-item note missing: {path}")]
    NoteMissing { path: String },
    #[error("{0}")]
    WriteStore(Box<dyn std::error::Error + Send + Sync>),
}

/// Deletes an open item after unlinking its index entry.
///
/// An unlink failure leaves the note untouched.
#[cqrsy::command]
pub fn execute<S>(
    cmd: &RemovePendingWorkItem,
    store: &S,
    projects: &ProjectRegistry,
) -> Result<RemovedItem, RemovePendingWorkError>
where
    S: AppDbStore<PendingWorkItem> + AppDbStore<IndexEntry>,
{
    let not_found = || RemovePendingWorkError::ItemNotFound { id: cmd.id.clone() };
    let Ok(id) = WorkItemId::try_new(&cmd.id) else {
        return Err(not_found());
    };
    let project = projects.project_for_id(&id).ok_or_else(not_found)?;
    let record = store_util::require_item(store, project, &id).map_err(|error| match error {
        LoadItemError::ItemNotFound { id } => RemovePendingWorkError::ItemNotFound { id },
        LoadItemError::Store(source) => RemovePendingWorkError::WriteStore(source),
    })?;
    if record.status != WorkItemStatus::Active {
        return Err(not_found());
    }
    if let Materialization::MissingNote { expected } = &record.materialization {
        return Err(RemovePendingWorkError::NoteMissing {
            path: expected.clone(),
        });
    }

    <S as AppDbStore<IndexEntry>>::delete(store, project, &id)
        .map_err(|error| RemovePendingWorkError::WriteStore(Box::new(error)))?;
    <S as AppDbStore<PendingWorkItem>>::delete(store, project, &id)
        .map_err(|error| RemovePendingWorkError::WriteStore(Box::new(error)))?;

    Ok(RemovedItem {
        id: id.as_ref().to_string(),
        project: project.as_ref().to_string(),
        title: record.title,
        deleted_path: PathBuf::from(record.locator),
        unlinked: record
            .placement
            .map(|placement| placement.index_path)
            .unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use pwf_domain::pending_work::{
        ProjectName, ProjectRegistry, Timestamp, WorkItemId, WorkItemStatus,
    };

    use super::{RemovePendingWorkError, RemovePendingWorkItem, execute};
    use crate::{
        IndexEntry, IndexEntryState, Materialization, PendingWorkItem, RecordId,
        testing::InMemoryStore,
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
        <InMemoryStore as crate::AppDbStore<IndexEntry>>::insert(
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

    #[test]
    fn remove_deletes_record_and_index_entry() {
        let store = staged(WorkItemStatus::Active);

        let removed = execute(&command("PWF-0001"), &store, &registry()).unwrap();

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

        let error = execute(&command("PWF-0001"), &store, &registry()).unwrap_err();

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

        let error = execute(&command("PWF-9999"), &store, &registry()).unwrap_err();

        assert!(matches!(
            error,
            RemovePendingWorkError::ItemNotFound { ref id } if id == "PWF-9999"
        ));
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

        let error = execute(&command("PWF-0001"), &store, &registry()).unwrap_err();

        assert!(matches!(
            error,
            RemovePendingWorkError::NoteMissing { ref path } if path == "/notes/pwf/PWF-0001.md"
        ));
        assert_eq!(
            error.to_string(),
            "Work-item note missing: /notes/pwf/PWF-0001.md"
        );
    }
}
