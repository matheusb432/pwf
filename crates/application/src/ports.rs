use std::path::Path;

use pwf_domain::pending_work::{ProjectName, Tags, Timestamp, WorkItemId, WorkItemStatus};

/// Defines the scope, identity, insertion, and patch types for a persisted record.
pub trait Record {
    type Scope;
    type Id;
    type New;
    type Patch;
}

/// Persists records by scope and identity without filtering, ordering, or pagination.
pub trait AppDbStore<R: Record>: Clone + Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;

    fn get(&self, scope: &R::Scope, id: &R::Id) -> Result<Option<R>, Self::Error>;
    /// Reads every record within `scope`, unfiltered and unordered.
    fn list(&self, scope: &R::Scope) -> Result<Vec<R>, Self::Error>;
    fn insert(&self, scope: &R::Scope, new: R::New) -> Result<R, Self::Error>;
    fn update(&self, scope: &R::Scope, id: &R::Id, patch: R::Patch) -> Result<(), Self::Error>;
    fn delete(&self, scope: &R::Scope, id: &R::Id) -> Result<(), Self::Error>;
}

/// A pending-work record's identity within its project.
///
/// Canonical items carry a [`WorkItemId`]. Inline prompts use a one-based ordinal; read handlers
/// combine it with the project as `<project>:<ordinal>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordId {
    Item(WorkItemId),
    Inline(usize),
}

impl RecordId {
    #[must_use]
    pub fn as_item(&self) -> Option<&WorkItemId> {
        match self {
            Self::Item(id) => Some(id),
            Self::Inline(_) => None,
        }
    }
}

/// Locates a record's open link by index display path and one-based line number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexPlacement {
    pub index_path: String,
    pub line: usize,
}

/// How the vault materializes a record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Materialization {
    NoteFile,
    /// An index link without a note file; `expected` is the platform-formatted diagnostic path.
    MissingNote {
        expected: String,
    },
    /// An inline prompt stored in the index without a note file.
    InlineLegacy,
}

/// A pending-work note's persisted state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingWorkItem {
    pub id: RecordId,
    pub title: String,
    pub status: WorkItemStatus,
    pub created: Option<Timestamp>,
    pub completed: Option<Timestamp>,
    pub commits: Option<String>,
    /// Preserves raw `tags:` frontmatter for lazy validation by tag-filtered reads.
    pub tags: Option<String>,
    pub effort: Option<String>,
    pub prereq: Option<String>,
    pub section: Option<String>,
    /// Preserves the note body below frontmatter verbatim.
    pub body: String,
    /// Preserves the raw note source byte-for-byte for `pwf show`.
    pub source: String,
    /// Stores the display path; writes relocate the record by scope and id.
    pub locator: String,
    /// Identifies the open index link used to read this record, when present.
    pub placement: Option<IndexPlacement>,
    pub materialization: Materialization,
}

impl Record for PendingWorkItem {
    type Scope = ProjectName;
    type Id = WorkItemId;
    type New = NewItem;
    type Patch = ItemPatch;
}

/// The shape used to insert a new [`PendingWorkItem`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewItem {
    pub prompt: String,
    pub title: Option<String>,
    pub created: Timestamp,
    pub section: Option<String>,
    pub prereq: Option<String>,
    pub effort: Option<u8>,
    pub tags: Option<Tags>,
}

/// The shape used to patch an existing [`PendingWorkItem`].
///
/// `None` leaves a field unchanged. Nested `Some(None)` clears nullable fields. The application
/// rejects empty patches before calling the adapter.
#[allow(clippy::option_option)]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemPatch {
    pub status: Option<WorkItemStatus>,
    pub completed: Option<Option<Timestamp>>,
    pub commits: Option<Option<String>>,
    pub body: Option<String>,
    pub title: Option<String>,
    pub prereq: Option<Option<String>>,
    pub effort: Option<u8>,
    pub tags: Option<Option<Tags>>,
}

/// A project index's per-item entry, tracking open/done state and section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexEntry {
    pub id: WorkItemId,
    pub state: IndexEntryState,
    /// Retains the raw stored label; the application owns normalization.
    pub section: String,
}

/// Records whether an index entry is open or completed on a date.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexEntryState {
    Open,
    Done(Timestamp),
}

impl Record for IndexEntry {
    type Scope = ProjectName;
    type Id = WorkItemId;
    // Insertions use upsert-by-value semantics.
    type New = IndexEntry;
    type Patch = IndexEntry;
}

/// Represents an `## <label>` section that can be listed or renamed.
///
/// [`IndexEntry`] upserts create missing sections implicitly. Direct section insertion and
/// deletion are unsupported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexSection {
    /// Retains the raw H2 label; the application owns normalization.
    pub label: String,
}

impl Record for IndexSection {
    type Scope = ProjectName;
    type Id = String;
    // Only update is supported; it renames the H2 label.
    type New = IndexSection;
    type Patch = IndexSection;
}

/// Reads raw note Markdown by path outside the record-keyed [`AppDbStore`] port.
///
/// Missing-note `show` operations use this seam to preserve the storage backend's read error.
pub trait NoteMarkdownSource: Clone + Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Raw markdown of the note at `path`, surfacing the backend's read failure.
    fn read_note_markdown(&self, path: &Path) -> Result<String, Self::Error>;
}
