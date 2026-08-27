use std::num::NonZeroUsize;

use pwf_models::{
    AppDate,
    project::Project,
    task::{BlockedBy, EffortTier, TaskId, TaskSection, TaskStatus, TaskTags, TaskTitle},
};
use pwf_wire::task::{RawTaskTags, TaskIndexPath, TaskNotePath};

/// Represents the optional `blocked_by` property after infrastructure parsing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum StoredBlockedBy {
    #[default]
    Absent,
    Valid(BlockedBy),
    Malformed {
        raw: String,
        reason: String,
    },
}

impl StoredBlockedBy {
    #[must_use]
    pub fn valid(&self) -> Option<&BlockedBy> {
        match self {
            Self::Valid(blocked_by) => Some(blocked_by),
            Self::Absent | Self::Malformed { .. } => None,
        }
    }
}

/// Locates a record's open link by index display path and one-based line number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexPlacement {
    pub index_path: TaskIndexPath,
    pub line: NonZeroUsize,
}

/// How the vault materializes a record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Materialization {
    NoteFile,
    /// An index link without a note file; `expected` is the platform-formatted diagnostic path.
    MissingNote {
        expected: TaskNotePath,
    },
}

/// Carries one raw task persistence record between application interactors and adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRecord {
    pub id: TaskId,
    pub title: String,
    pub status: TaskStatus,
    pub created: Option<AppDate>,
    pub completed: Option<AppDate>,
    pub commits: Option<String>,
    /// Preserves raw `tags:` frontmatter for lazy validation by tag-filtered reads.
    pub tags: Option<RawTaskTags>,
    pub effort: Option<String>,
    /// Carries typed or malformed `blocked_by` metadata for boundary-specific handling.
    pub blocked_by: StoredBlockedBy,
    pub section: Option<TaskSection>,
    /// Preserves the note body below frontmatter verbatim.
    pub body: String,
    /// Preserves the raw note source byte-for-byte for `pwf task get`.
    pub source: String,
    /// Stores the display path; writes relocate the record by scope and id.
    pub locator: TaskNotePath,
    /// Identifies the open index link used to read this record, when present.
    pub placement: Option<IndexPlacement>,
    pub materialization: Materialization,
}

/// The shape used to insert a new [`TaskRecord`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTask {
    pub body: String,
    pub title: TaskTitle,
    pub created: AppDate,
    pub section: Option<TaskSection>,
    pub blocked_by: Option<BlockedBy>,
    pub effort: Option<EffortTier>,
    pub tags: Option<TaskTags>,
}

/// Selects how a patch changes one nullable field.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum NullablePatch<T> {
    #[default]
    Unchanged,
    Clear,
    Set(T),
}

impl<T> NullablePatch<T> {
    /// Maps a replacement value without changing the patch decision.
    pub fn map<U>(self, map: impl FnOnce(T) -> U) -> NullablePatch<U> {
        match self {
            Self::Unchanged => NullablePatch::Unchanged,
            Self::Clear => NullablePatch::Clear,
            Self::Set(value) => NullablePatch::Set(map(value)),
        }
    }
}

/// The shape used to patch an existing [`TaskRecord`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskPatch {
    pub status: Option<TaskStatus>,
    pub completed: NullablePatch<AppDate>,
    pub commits: NullablePatch<String>,
    pub body: Option<String>,
    pub title: Option<TaskTitle>,
    pub blocked_by: NullablePatch<BlockedBy>,
    pub effort: NullablePatch<EffortTier>,
    pub tags: NullablePatch<TaskTags>,
}

/// A project index's per-task entry, tracking open/done state and section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexEntry {
    pub id: TaskId,
    pub state: IndexEntryState,
    /// Retains the raw stored label; the application owns normalization.
    pub section: Option<TaskSection>,
}

/// Records whether an index entry is open or completed on a date.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexEntryState {
    Open,
    Done(Option<AppDate>),
}

pub trait TaskStore: Clone + Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;

    fn get(&self, project: &Project, id: &TaskId) -> Result<Option<TaskRecord>, Self::Error>;
    fn list(&self, project: &Project) -> Result<Vec<TaskRecord>, Self::Error>;
    fn next_id(&self, project: &Project) -> Result<TaskId, Self::Error>;
    fn insert(
        &self,
        project: &Project,
        id: &TaskId,
        new: NewTask,
    ) -> Result<TaskRecord, Self::Error>;
    fn update(&self, project: &Project, id: &TaskId, patch: TaskPatch) -> Result<(), Self::Error>;
    fn delete(&self, project: &Project, id: &TaskId) -> Result<(), Self::Error>;
}

pub trait IndexEntryStore: Clone + Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;

    fn list_index_entries(&self, project: &Project) -> Result<Vec<IndexEntry>, Self::Error>;
    fn upsert_index_entry(&self, project: &Project, entry: IndexEntry) -> Result<(), Self::Error>;
    fn delete_index_entry(&self, project: &Project, id: &TaskId) -> Result<(), Self::Error>;
}

pub trait IndexSectionStore: Clone + Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;

    fn list_index_sections(&self, project: &Project) -> Result<Vec<TaskSection>, Self::Error>;
    fn rename_index_section(
        &self,
        project: &Project,
        current_label: &TaskSection,
        new_label: &TaskSection,
    ) -> Result<(), Self::Error>;
}
