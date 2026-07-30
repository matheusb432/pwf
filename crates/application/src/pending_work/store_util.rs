//! Shared item loading and creation over the persistence ports.

use pwf_models::pending_work::{ProjectName, WorkItemId};

use super::{add_pending_work_item::CreateItemError, enrich::normalize_section_label};
use crate::ports::{
    AppRecordStore, IndexEntry, IndexEntryState, IndexSection, NewItem, PendingWorkRecord,
};

/// Reports failures while loading a required item.
#[derive(Debug, thiserror::Error)]
pub enum LoadItemError {
    #[error("Open pending-work item not found: {id}")]
    ItemNotFound { id: String },
    #[error("{0}")]
    Store(Box<dyn std::error::Error + Send + Sync>),
}

/// Canonical labels that materialize as dedicated H2 sections.
const SECTION_LABELS: [&str; 3] = ["Future", "Human", "Low-prio"];

/// Contains a created record and the new H2 section, if one was needed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedItem {
    pub record: PendingWorkRecord,
    pub created_section: Option<String>,
}

/// Strips the frontmatter parser's retained leading blank line before a body rewrite.
#[must_use]
pub(super) fn body_region(body: &str) -> &str {
    body.strip_prefix('\n').unwrap_or(body)
}

/// Reads a required item within a project.
pub(super) fn require_item<S>(
    store: &S,
    project: &ProjectName,
    id: &WorkItemId,
) -> Result<PendingWorkRecord, LoadItemError>
where
    S: AppRecordStore<PendingWorkRecord>,
{
    <S as AppRecordStore<PendingWorkRecord>>::get(store, project, id)
        .map_err(|error| LoadItemError::Store(Box::new(error)))?
        .ok_or_else(|| LoadItemError::ItemNotFound {
            id: id.as_ref().to_string(),
        })
}

/// Inserts a note record, then upserts its open index entry.
///
/// # Panics
///
/// Panics if the store's `insert` violates its contract by returning a record
/// without a canonical [`WorkItemId`].
pub(crate) fn create_item<S>(
    store: &S,
    project: &ProjectName,
    new: NewItem,
) -> Result<CreatedItem, CreateItemError>
where
    S: AppRecordStore<PendingWorkRecord>
        + AppRecordStore<IndexEntry>
        + AppRecordStore<IndexSection>,
{
    let target_section = new.section.clone();
    // Read sections before writing so an invalid index leaves no orphaned note.
    let existing = <S as AppRecordStore<IndexSection>>::list(store, project)
        .map_err(|error| CreateItemError::ReadSections(Box::new(error)))?;
    let created_section = target_section
        .as_deref()
        .filter(|label| SECTION_LABELS.contains(label))
        .filter(|label| {
            !existing
                .iter()
                .any(|section| normalize_section_label(&section.label) == *label)
        })
        .map(str::to_string);

    let record = <S as AppRecordStore<PendingWorkRecord>>::insert(store, project, new)
        .map_err(|error| CreateItemError::InsertRecord(Box::new(error)))?;
    let id = record
        .id
        .as_item()
        .expect("inserted record carries a canonical id")
        .clone();
    <S as AppRecordStore<IndexEntry>>::insert(
        store,
        project,
        IndexEntry {
            id,
            state: IndexEntryState::Open,
            // Preserve the raw label; the adapter owns placement.
            section: target_section.unwrap_or_default(),
        },
    )
    .map_err(|error| CreateItemError::InsertIndex {
        project: project.clone(),
        created_section: created_section.clone(),
        source: Box::new(error),
    })?;

    Ok(CreatedItem {
        record,
        created_section,
    })
}

#[cfg(test)]
mod tests {
    use pwf_models::pending_work::{ProjectName, TaskTitle, Timestamp, WorkItemId};

    use super::{LoadItemError, create_item, require_item};
    use crate::{
        IndexEntryState, NewItem, RecordId, pending_work::resolve::testing::staged,
        testing::InMemoryStore,
    };

    fn pwf() -> ProjectName {
        ProjectName::try_new("pwf").unwrap()
    }

    fn new_item(section: Option<&str>) -> NewItem {
        NewItem {
            prompt: "do the thing".to_string(),
            title: TaskTitle::try_new("ship it").unwrap(),
            created: Timestamp::new("2026-07-15"),
            section: section.map(str::to_string),
            prereq: None,
            effort: None,
            tags: None,
        }
    }

    fn staged_store() -> InMemoryStore {
        InMemoryStore::default().with_prefix("pwf", "PWF")
    }

    #[test]
    fn create_item_inserts_record_and_open_index_entry() {
        let store = staged_store();

        let created = create_item(&store, &pwf(), new_item(None)).unwrap();

        let id = WorkItemId::try_new("PWF-0001").unwrap();
        assert_eq!(created.record.id, RecordId::Item(id.clone()));
        assert_eq!(created.created_section, None);
        let items = store.items("pwf");
        assert_eq!(items.len(), 1, "record must be inserted");
        assert_eq!(items[0].id, RecordId::Item(id.clone()));
        let entries = store.entries("pwf");
        assert_eq!(entries.len(), 1, "open index entry must be upserted");
        assert_eq!(entries[0].id, id);
        assert_eq!(entries[0].state, IndexEntryState::Open);
        assert_eq!(entries[0].section, "");
    }

    #[test]
    fn create_item_reports_created_section_when_region_absent() {
        let store = staged_store();

        let created = create_item(&store, &pwf(), new_item(Some("Human"))).unwrap();

        assert_eq!(created.created_section.as_deref(), Some("Human"));
        assert_eq!(store.entries("pwf")[0].section, "Human");
    }

    #[test]
    fn create_item_does_not_report_existing_empty_section_region() {
        let store = staged_store().with_sections("pwf", &["Human"]);

        let created = create_item(&store, &pwf(), new_item(Some("Human"))).unwrap();

        assert_eq!(created.created_section, None);
    }

    #[test]
    fn create_item_matches_section_aliases_like_the_legacy_read_headers() {
        let store = staged_store().with_sections("pwf", &["Futuro"]);

        let created = create_item(&store, &pwf(), new_item(Some("Future"))).unwrap();

        assert_eq!(created.created_section, None);
    }

    #[test]
    fn require_item_returns_staged_record() {
        let (store, _registry) = staged();
        let id = WorkItemId::try_new("PWF-0001").unwrap();

        let record = require_item(&store, &pwf(), &id).unwrap();

        assert_eq!(record.id, RecordId::Item(id));
    }

    #[test]
    fn require_item_missing_maps_to_legacy_not_found_display() {
        let (store, _registry) = staged();
        let id = WorkItemId::try_new("PWF-9999").unwrap();

        let error = require_item(&store, &pwf(), &id).unwrap_err();

        assert!(matches!(
            error,
            LoadItemError::ItemNotFound { ref id } if id == "PWF-9999"
        ));
        assert_eq!(
            error.to_string(),
            "Open pending-work item not found: PWF-9999"
        );
    }

    #[test]
    fn created_item_carries_record_and_section_fact() {
        let store = staged_store();
        let created = create_item(&store, &pwf(), new_item(Some("Low-prio"))).unwrap();
        assert_eq!(
            created.record.id,
            RecordId::Item(WorkItemId::try_new("PWF-0001").unwrap())
        );
        assert_eq!(created.record.title, "ship it");
        assert_eq!(created.created_section.as_deref(), Some("Low-prio"));
    }
}
