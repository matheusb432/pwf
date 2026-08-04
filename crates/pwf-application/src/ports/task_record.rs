use pwf_models::{
    project::Project,
    task::{EffortTier, Prerequisites, Tags, TaskId, TaskStatus, TaskTitle, Timestamp},
};

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
}

/// Carries one raw task persistence record between application interactors and adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRecord {
    pub id: TaskId,
    pub title: String,
    pub status: TaskStatus,
    pub created: Option<Timestamp>,
    pub completed: Option<Timestamp>,
    pub commits: Option<String>,
    /// Preserves raw `tags:` frontmatter for lazy validation by tag-filtered reads.
    pub tags: Option<String>,
    pub effort: Option<String>,
    /// Preserves raw `prereq:` frontmatter for validation at read boundaries.
    pub prereq: Option<String>,
    pub section: Option<String>,
    /// Preserves the note body below frontmatter verbatim.
    pub body: String,
    /// Preserves the raw note source byte-for-byte for `pwf task show`.
    pub source: String,
    /// Stores the display path; writes relocate the record by scope and id.
    pub locator: String,
    /// Identifies the open index link used to read this record, when present.
    pub placement: Option<IndexPlacement>,
    pub materialization: Materialization,
}

/// The shape used to insert a new [`TaskRecord`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTask {
    pub prompt: String,
    pub title: TaskTitle,
    pub created: Timestamp,
    pub section: Option<String>,
    pub prereq: Option<Prerequisites>,
    pub effort: Option<EffortTier>,
    pub tags: Option<Tags>,
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
    pub completed: NullablePatch<Timestamp>,
    pub commits: NullablePatch<String>,
    pub body: Option<String>,
    pub title: Option<TaskTitle>,
    pub prereq: NullablePatch<Prerequisites>,
    pub effort: Option<EffortTier>,
    pub tags: NullablePatch<Tags>,
}

/// A project index's per-task entry, tracking open/done state and section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexEntry {
    pub id: TaskId,
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

/// Represents an `## <label>` section that can be listed or renamed.
///
/// [`IndexEntry`] upserts create missing sections implicitly. Direct section insertion and
/// deletion are unsupported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexSection {
    /// Retains the raw H2 label; the application owns normalization.
    pub label: String,
}

pub trait TaskStore: Clone + Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;

    fn get(&self, project: &Project, id: &TaskId) -> Result<Option<TaskRecord>, Self::Error>;
    fn list(&self, project: &Project) -> Result<Vec<TaskRecord>, Self::Error>;
    fn insert(&self, project: &Project, new: NewTask) -> Result<TaskRecord, Self::Error>;
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

    fn list_index_sections(&self, project: &Project) -> Result<Vec<IndexSection>, Self::Error>;
    fn rename_index_section(
        &self,
        project: &Project,
        current_label: &str,
        new_label: &str,
    ) -> Result<(), Self::Error>;
}
