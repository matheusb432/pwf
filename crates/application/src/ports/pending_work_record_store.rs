use pwf_models::pending_work::{
    EffortTier, ProjectName, Tags, TaskTitle, Timestamp, WorkItemId, WorkItemStatus,
};

use super::Record;

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

/// Carries one raw pending-work persistence record between application operations and adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingWorkRecord {
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

impl Record for PendingWorkRecord {
    type Scope = ProjectName;
    type Id = WorkItemId;
    type New = NewItem;
    type Patch = ItemPatch;
}

/// The shape used to insert a new [`PendingWorkRecord`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewItem {
    pub prompt: String,
    pub title: TaskTitle,
    pub created: Timestamp,
    pub section: Option<String>,
    pub prereq: Option<String>,
    pub effort: Option<EffortTier>,
    pub tags: Option<Tags>,
}

/// The shape used to patch an existing [`PendingWorkRecord`].
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
    pub title: Option<TaskTitle>,
    pub prereq: Option<Option<String>>,
    pub effort: Option<EffortTier>,
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
