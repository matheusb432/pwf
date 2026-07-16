use std::path::Path;

use pwf_domain::pending_work::{ProjectName, Tags, Timestamp, WorkItemId, WorkItemStatus};

/// Marker trait for a persisted-state DTO keyed by a `(Scope, Id)` pair, with
/// its own new/patch shapes for insert and update.
///
/// Implemented by each record kind the generic [`AppDbStore`] can store
/// ([`PendingWorkItem`], [`IndexEntry`], and [`IndexSection`]).
pub trait Record {
    /// The scope an instance of this record lives under (e.g. a project).
    type Scope;
    /// The identity of a single record within its scope.
    type Id;
    /// The shape used to insert a new record.
    type New;
    /// The shape used to patch an existing record.
    type Patch;
}

/// Generic, record-keyed persistence port.
///
/// Deliberately minimal and transitional: no filtering/ordering/pagination/
/// query params. The SQL era replaces the impl, not the trait.
pub trait AppDbStore<R: Record>: Clone + Send + Sync + 'static {
    /// The error type surfaced by this store's operations.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Reads a single record by id within `scope`, if it exists.
    fn get(&self, scope: &R::Scope, id: &R::Id) -> Result<Option<R>, Self::Error>;
    /// Reads every record within `scope`, unfiltered and unordered.
    fn list(&self, scope: &R::Scope) -> Result<Vec<R>, Self::Error>;
    /// Inserts a new record within `scope`, returning the stored record.
    fn insert(&self, scope: &R::Scope, new: R::New) -> Result<R, Self::Error>;
    /// Applies `patch` to the record identified by `id` within `scope`.
    fn update(&self, scope: &R::Scope, id: &R::Id, patch: R::Patch) -> Result<(), Self::Error>;
    /// Deletes the record identified by `id` within `scope`.
    fn delete(&self, scope: &R::Scope, id: &R::Id) -> Result<(), Self::Error>;
}

/// A pending-work record's identity within its project.
///
/// Canonical items carry a typed [`WorkItemId`]; legacy inline prompts (a
/// backtick-session checkbox line in the index, no note file) have only a
/// 1-based ordinal — their display id is `<project>:<ordinal>`, composed by
/// the read handlers that know the scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordId {
    Item(WorkItemId),
    Inline(usize),
}

impl RecordId {
    /// The canonical id, when this record has one.
    #[must_use]
    pub fn as_item(&self) -> Option<&WorkItemId> {
        match self {
            Self::Item(id) => Some(id),
            Self::Inline(_) => None,
        }
    }
}

/// Where a record's entry sits in its project index — the index note's display
/// path and the entry's 1-based line number. Representation facts the list view
/// renders (`note: <index>:<line>`) and remove reports (`unlinked:`); populated
/// whenever the index holds an open link for the record, `None` otherwise
/// (e.g. closed records read directly off their note file).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexPlacement {
    pub index_path: String,
    pub line: usize,
}

/// How the vault materializes a record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Materialization {
    /// An index wikilink backed by its own note file.
    NoteFile,
    /// An index wikilink whose note file is missing; `expected` is the
    /// diagnostic-facing path the note should occupy (platform display form,
    /// where `locator` stays the normalized display path).
    MissingNote { expected: String },
    /// A legacy inline prompt line inside the index (no note file).
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
    /// Raw `tags:` frontmatter, verbatim — validated lazily (only when a tag
    /// filter is requested), so a corrupt value never fails an unfiltered read.
    pub tags: Option<String>,
    pub effort: Option<String>,
    pub prereq: Option<String>,
    pub section: Option<String>,
    /// Note body below frontmatter, verbatim.
    pub body: String,
    /// Full raw note text — `pwf show`'s byte-exact output contract.
    pub source: String,
    /// Display path; the adapter re-locates by `(scope, id)` on writes.
    pub locator: String,
    /// Index entry placement, when this record was read through the index.
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
/// Every field `None`/`false` = no-op; the adapter never rejects an empty
/// patch — application validates (`NothingToUpdate`) before calling.
// `Some(None)` = clear is the port's tri-state patch convention (no-op / clear /
// set); a wrapper enum would obscure that this is an Option-patch field.
#[allow(clippy::option_option)]
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemPatch {
    pub status: Option<WorkItemStatus>,
    /// `Some(None)` clears.
    pub completed: Option<Option<Timestamp>>,
    /// `Some(None)` clears.
    pub commits: Option<Option<String>>,
    /// Full replacement text.
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
    /// RAW stored label (e.g. "Futuro") — application decides normalization.
    pub section: String,
}

/// Whether an [`IndexEntry`] is open or done (with its completion date).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexEntryState {
    Open,
    Done(Timestamp),
}

impl Record for IndexEntry {
    type Scope = ProjectName;
    type Id = WorkItemId;
    // Upsert-by-value semantics.
    type New = IndexEntry;
    type Patch = IndexEntry;
}

/// A section region of a project index (an `## <label>` H2 header).
///
/// List-plus-rename record kind: the adapter reports which section regions
/// exist so application policy can decide section-existence questions (e.g.
/// `add`'s created-section diagnostic), and `update` renames a section label in
/// place (the futuro-normalization seam). Creation happens implicitly when an
/// [`IndexEntry`] upsert targets a missing section, and sections are never
/// deleted — `insert`/`delete` through the port are unsupported by design.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexSection {
    /// RAW stored H2 label (e.g. "Futuro") — application decides normalization.
    pub label: String,
}

impl Record for IndexSection {
    type Scope = ProjectName;
    type Id = String;
    // `insert`/`delete` reject (sections are created implicitly on an
    // `IndexEntry` upsert, never deleted); `update` is the one supported write —
    // the representation-only label rename that normalizes a `## Futuro` header.
    type New = IndexSection;
    type Patch = IndexSection;
}

/// The storage layer's raw-note-read surface, by path rather than by
/// `(scope, id)` — deliberately outside the generic [`AppDbStore`] record port.
///
/// The single remaining seam onto the storage backend's own read failure:
/// `pwf show` / `pwf resolve --show` replays a missing-note wikilink's rejection
/// by attempting the real read of the expected note, so the surfaced error is
/// *literally* the storage error (`Cannot read item file: <io error>`) rather
/// than a reconstructed lookalike. Its permanence is why it outlived the
/// transitional `LegacyAppDbStore` (retired in PWF-0123 Task 4.1).
pub trait NoteMarkdownSource: Clone + Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Raw markdown of the note at `path`, surfacing the backend's read failure.
    fn read_note_markdown(&self, path: &Path) -> Result<String, Self::Error>;
}
