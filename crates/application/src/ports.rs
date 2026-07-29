mod app_db_store;
mod app_record_store;
mod clock;
mod project_task_files_client;
mod project_task_location_client;

use std::path::Path;

pub use app_db_store::AppDbStore;
#[cfg(test)]
pub(crate) use app_db_store::TestDatabase;
pub use app_record_store::{AppRecordStore, Record};
pub use clock::Clock;
pub use project_task_files_client::{
    ProjectTaskFilesClient, ProjectTaskFilesRenameCommit, StagedProjectTaskFilesRename,
};
pub use project_task_location_client::ProjectTaskLocationClient;
use pwf_models::{
    note::{NoteId, ProjectNote},
    pending_work::{EffortTier, ProjectName, Tags, Timestamp, WorkItemId, WorkItemStatus},
};

impl Record for ProjectNote {
    type Scope = ProjectName;
    type Id = NoteId;
    type New = NewProjectNote;
    type Patch = ProjectNotePatch;
}

/// Supplies the fields required to persist a new project note.
///
/// # Examples
///
/// ```
/// use pwf_application::NewProjectNote;
/// use pwf_models::{note::NoteId, pending_work::Timestamp};
///
/// let note = NewProjectNote {
///     id: NoteId::try_new("PWF-NOTE-0001").unwrap(),
///     message: "remember milk".to_string(),
///     created: Timestamp::new("2026-07-26"),
/// };
/// assert_eq!(note.created.as_str(), "2026-07-26");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewProjectNote {
    /// Identifies the new note within its project.
    pub id: NoteId,
    /// Contains the trimmed note message.
    pub message: String,
    /// Records the authored creation date.
    pub created: Timestamp,
}

/// Replaces the mutable fields of one project note.
///
/// # Examples
///
/// ```
/// use pwf_application::ProjectNotePatch;
///
/// let patch = ProjectNotePatch {
///     message: "remember oat milk".to_string(),
/// };
/// assert_eq!(patch.message, "remember oat milk");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectNotePatch {
    /// Replaces the note body message.
    pub message: String,
}

/// Inspects project-note representation facts required by note operations.
///
/// # Examples
///
/// ```
/// use pwf_application::{AppRecordStore, ProjectNoteStore};
/// use pwf_models::{
///     note::{NoteId, ProjectNote},
///     pending_work::ProjectName,
/// };
///
/// # fn exists<S>(
/// #     store: &S,
/// #     project: &ProjectName,
/// #     id: &NoteId,
/// # ) -> Result<bool, <S as AppRecordStore<ProjectNote>>::Error>
/// # where
/// #     S: ProjectNoteStore,
/// # {
/// store.note_exists(project, id)
/// # }
/// ```
pub trait ProjectNoteStore: AppRecordStore<ProjectNote> {
    /// Returns whether the note's exact representation path exists.
    ///
    /// # Errors
    ///
    /// Returns the storage adapter error when representation existence cannot be inspected.
    fn note_exists(
        &self,
        project: &ProjectName,
        id: &NoteId,
    ) -> Result<bool, <Self as AppRecordStore<ProjectNote>>::Error>;
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
    pub title: String,
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
    pub title: Option<String>,
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

/// Reads raw note Markdown by path outside the record-keyed [`AppRecordStore`] port.
///
/// Missing-note `show` operations use this seam to preserve the storage backend's read error.
pub trait NoteMarkdownSource: Clone + Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Reads raw Markdown from `path`.
    ///
    /// # Errors
    ///
    /// Returns the backend error when the note cannot be read.
    fn read_note_markdown(&self, path: &Path) -> Result<String, Self::Error>;
}
