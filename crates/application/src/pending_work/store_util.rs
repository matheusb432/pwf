//! Shared load/create utilities over the generic port — the pieces more than
//! one pending-work handler orchestrates with (`add`, the close handlers, and
//! done's `--review` path).

use pwf_domain::pending_work::{ProjectName, WorkItemId};

use super::enrich::normalize_section_label;
use crate::ports::{
    AppDbStore, IndexEntry, IndexEntryState, IndexSection, NewItem, PendingWorkItem,
};

/// A boxed adapter error, kept concrete inside the box so CLI diagnostics can
/// still downcast to the store's error type (e.g. the created-section payload
/// on a failed index write).
pub type StoreError = Box<dyn std::error::Error + Send + Sync>;

/// Load-or-fail on the generic port.
#[derive(Debug, thiserror::Error)]
pub enum LoadItemError {
    /// Verbatim former infra `ItemNotFound` display (PWF-0123 error-string
    /// relocation).
    #[error("Open pending-work item not found: {id}")]
    ItemNotFound { id: String },
    #[error("{0}")]
    Store(StoreError),
}

/// The exact labels the vault materializes as dedicated `##` sections — the
/// canonical `--section` values. Application policy: which labels are
/// section-worthy is decided here; the adapter only maps placement.
const SECTION_LABELS: [&str; 3] = ["Future", "Human", "Low-prio"];

/// What [`create_item`] wrote: the inserted record plus the created-section
/// fact — the target `--section` label when no matching H2 region existed
/// before the write (the data behind the CLI's created-section diagnostic).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedItem {
    pub record: PendingWorkItem,
    pub created_section: Option<String>,
}

/// Strips the leading blank line the frontmatter parser retains on a note body,
/// so re-wrapping it through the adapter's `replace_body` reproduces the note
/// verbatim. Shared by the `update` and close (`done`/`cancel`) handlers.
#[must_use]
pub fn body_region(body: &str) -> &str {
    body.strip_prefix('\n').unwrap_or(body)
}

/// Reads `id` within `project`, failing with the legacy not-found display when
/// no record exists.
pub fn require_item<S>(
    store: &S,
    project: &ProjectName,
    id: &WorkItemId,
) -> Result<PendingWorkItem, LoadItemError>
where
    S: AppDbStore<PendingWorkItem>,
{
    <S as AppDbStore<PendingWorkItem>>::get(store, project, id)
        .map_err(|error| LoadItemError::Store(Box::new(error)))?
        .ok_or_else(|| LoadItemError::ItemNotFound {
            id: id.as_ref().to_string(),
        })
}

/// Creates a pending-work item: inserts the record (note only), then upserts
/// its open index entry — reporting whether the entry's target section region
/// had to be created.
///
/// # Panics
///
/// Panics if the store's `insert` violates its contract by returning a record
/// without a canonical [`WorkItemId`].
pub fn create_item<S>(
    store: &S,
    project: &ProjectName,
    new: NewItem,
) -> Result<CreatedItem, StoreError>
where
    S: AppDbStore<PendingWorkItem> + AppDbStore<IndexEntry> + AppDbStore<IndexSection>,
{
    let target_section = new.section.clone();
    // The section listing doubles as the legacy pre-write index read: it runs
    // before the note insert, so a corrupt/mismatched index fails the create
    // with nothing written.
    let existing = <S as AppDbStore<IndexSection>>::list(store, project)
        .map_err(|error| -> StoreError { Box::new(error) })?;
    let created_section = target_section
        .as_deref()
        .filter(|label| SECTION_LABELS.contains(label))
        .filter(|label| {
            !existing
                .iter()
                .any(|section| normalize_section_label(&section.label) == *label)
        })
        .map(str::to_string);

    let record = <S as AppDbStore<PendingWorkItem>>::insert(store, project, new)
        .map_err(|error| -> StoreError { Box::new(error) })?;
    let id = record
        .id
        .as_item()
        .expect("inserted record carries a canonical id")
        .clone();
    <S as AppDbStore<IndexEntry>>::insert(
        store,
        project,
        IndexEntry {
            id,
            state: IndexEntryState::Open,
            // RAW pass-through: placement (including the non-canonical-label
            // general fallback) is the adapter's representation decision.
            section: target_section.unwrap_or_default(),
        },
    )
    .map_err(|error| -> StoreError { Box::new(error) })?;

    Ok(CreatedItem {
        record,
        created_section,
    })
}

#[cfg(test)]
mod tests {
    use pwf_domain::pending_work::{ProjectName, Timestamp, WorkItemId};

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
            title: Some("ship it".to_string()),
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

    /// The latent-regression pin: a section header that exists with zero
    /// entries must NOT be reported as created.
    #[test]
    fn create_item_does_not_report_existing_empty_section_region() {
        let store = staged_store().with_sections("pwf", &["Human"]);

        let created = create_item(&store, &pwf(), new_item(Some("Human"))).unwrap();

        assert_eq!(created.created_section, None);
    }

    /// Legacy alias parity: a `## Futuro` region satisfies `--section future`
    /// (same alias set as the adapter's read headers).
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
