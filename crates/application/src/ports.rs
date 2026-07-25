mod app_db_store;
mod app_record_store;

use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};

pub use app_db_store::AppDbStore;
pub use app_record_store::{AppRecordStore, Record};
use pwf_domain::{
    handoff::HandoffStatus,
    pending_work::{ProjectName, Tags, Timestamp, WorkItemId, WorkItemStatus},
};

/// Locates handoff records beneath one repository root.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct HandoffScope {
    /// Root containing `docs/handoffs`.
    pub repository_root: PathBuf,
}

/// Describes whether a repository and its active handoff directory exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandoffDocumentScopePresence {
    /// The configured repository root does not exist.
    RepositoryMissing,
    /// The repository exists without `docs/handoffs`.
    HandoffDirectoryMissing,
    /// The active handoff path exists and is a directory.
    Present,
}

/// Identifies a handoff file and its storage location.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct HandoffDocumentIdentifier {
    /// Markdown file name without parent directories.
    pub file_name: String,
    /// Active or archived location containing the file.
    pub location: HandoffLocation,
}

/// Selects the active or archived handoff directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HandoffLocation {
    /// The live `docs/handoffs` directory.
    Active,
    /// The immutable `docs/handoffs/archived` directory.
    Archived,
}

/// A parsed handoff document with its raw representation and file metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoffDocument {
    /// Record identity within the repository.
    pub identifier: HandoffDocumentIdentifier,
    /// Directory location represented separately for operation results.
    pub location: HandoffLocation,
    /// Parsed managed project, when valid.
    pub project: Option<ProjectName>,
    /// First level-one heading or file-stem fallback.
    pub title: String,
    /// Parsed lifecycle state, when valid.
    pub status: Option<HandoffStatus>,
    /// Authored creation value, when present.
    pub created: Option<Timestamp>,
    /// Authored completion value, when present.
    pub completed: Option<Timestamp>,
    /// Unvalidated `pw:` value retained for operation-level validation.
    pub pending_work_identifier_raw: Option<String>,
    /// Number of checked goal boxes.
    pub goals_completed: usize,
    /// Total number of goal boxes.
    pub goals_total: usize,
    /// Markdown below frontmatter.
    pub body: String,
    /// Complete source preserved byte-for-byte.
    pub source: String,
    /// Concrete path used by the adapter.
    pub locator: PathBuf,
    /// Filesystem modification time used by newest-handoff selection.
    pub modified_timestamp: SystemTime,
}

impl Record for HandoffDocument {
    type Scope = HandoffScope;
    type Id = HandoffDocumentIdentifier;
    type New = NewHandoffDocument;
    type Patch = HandoffPatch;
}

/// Extends handoff record storage with representation facts needed for lifecycle recovery.
pub trait HandoffDocumentStore: AppRecordStore<HandoffDocument> {
    /// Inspects repository and active-directory presence without listing documents.
    ///
    /// # Errors
    ///
    /// Returns the adapter error when either path cannot be inspected.
    fn scope_presence(
        &self,
        scope: &HandoffScope,
    ) -> Result<HandoffDocumentScopePresence, <Self as AppRecordStore<HandoffDocument>>::Error>;

    /// Returns whether the exact representation path exists without parsing it.
    ///
    /// # Errors
    ///
    /// Returns the adapter error when the path cannot be inspected.
    fn document_exists(
        &self,
        scope: &HandoffScope,
        identifier: &HandoffDocumentIdentifier,
    ) -> Result<bool, <Self as AppRecordStore<HandoffDocument>>::Error>;

    /// Reads handoff documents from exactly one lifecycle directory.
    ///
    /// # Errors
    ///
    /// Returns the adapter error when the selected directory cannot be read.
    fn list_location(
        &self,
        scope: &HandoffScope,
        location: HandoffLocation,
    ) -> Result<Vec<HandoffDocument>, <Self as AppRecordStore<HandoffDocument>>::Error>;

    /// Restores a moved document before removing its exact opposite-location record.
    ///
    /// # Errors
    ///
    /// Returns the adapter error when the snapshot cannot be restored.
    fn restore_document_after_move(
        &self,
        scope: &HandoffScope,
        snapshot: &HandoffDocument,
    ) -> Result<(), <Self as AppRecordStore<HandoffDocument>>::Error>;

    /// Restores a deleted document without changing any opposite-location record.
    ///
    /// # Errors
    ///
    /// Returns the adapter error when the snapshot cannot be restored.
    fn restore_document_after_delete(
        &self,
        scope: &HandoffScope,
        snapshot: &HandoffDocument,
    ) -> Result<(), <Self as AppRecordStore<HandoffDocument>>::Error>;
}

/// Data required to create an active handoff document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewHandoffDocument {
    /// Markdown file name without parent directories.
    pub file_name: String,
    /// Managed project owning the repository.
    pub project: ProjectName,
    /// Handoff heading.
    pub title: String,
    /// Authored creation value.
    pub created: Timestamp,
    /// Markdown body below frontmatter.
    pub body: String,
    /// Optional linked pending-work identifier.
    pub pending_work_identifier: Option<WorkItemId>,
}

/// Fields changed by a handoff lifecycle operation.
#[allow(clippy::option_option)]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HandoffPatch {
    /// Moves the document when present.
    pub location: Option<HandoffLocation>,
    /// Replaces lifecycle status when present.
    pub status: Option<HandoffStatus>,
    /// Replaces or clears completion metadata when present.
    pub completed: Option<Option<Timestamp>>,
    /// Sets the pending-work link when present.
    pub pending_work_identifier: Option<WorkItemId>,
    /// Replaces the body when present.
    pub body: Option<String>,
}

/// The persisted handoff ledger Markdown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoffLedger {
    /// Complete ledger Markdown.
    pub source: String,
    /// Concrete ledger path used by the adapter.
    pub locator: PathBuf,
}

impl Record for HandoffLedger {
    type Scope = HandoffScope;
    type Id = HandoffLedgerIdentifier;
    type New = HandoffLedgerWrite;
    type Patch = HandoffLedgerWrite;
}

/// Singleton identity for one repository's handoff ledger.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HandoffLedgerIdentifier;

/// Rows used to replace one derived handoff ledger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoffLedgerWrite {
    /// Rows in final display order.
    pub rows: Vec<HandoffLedgerRow>,
}

/// One active handoff row in the derived ledger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandoffLedgerRow {
    /// Validated identifier or legacy file-stem fallback.
    pub pending_work_identifier: String,
    /// Handoff heading.
    pub title: String,
    /// Relative Markdown link target.
    pub file_name: String,
    /// Number of checked goals.
    pub goals_completed: usize,
    /// Total number of goals.
    pub goals_total: usize,
    /// Authored creation value used for ordering and display.
    pub created: String,
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
