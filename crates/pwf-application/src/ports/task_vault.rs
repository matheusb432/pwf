use std::{collections::BTreeSet, num::NonZeroUsize};

use pwf_models::{
    project::Project,
    revision::ContentRevision,
    task::{
        BlockedBy, EffortTier, PriorityTier, TaskId, TaskSection, TaskStatus, TaskTags,
        TaskTimestamp, TaskTitle,
    },
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
    pub created_at: Option<TaskTimestamp>,
    pub completed_at: Option<TaskTimestamp>,
    pub commits: Option<String>,
    /// Preserves raw `tags:` frontmatter for lazy validation by tag-filtered reads.
    pub tags: Option<RawTaskTags>,
    pub effort: Option<String>,
    pub priority: Option<String>,
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
    /// Opaque revision of the exact persisted file bytes backing this record.
    pub revision: ContentRevision,
}

/// The shape used to insert a new [`TaskRecord`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTask {
    pub body: String,
    pub title: TaskTitle,
    pub created_at: TaskTimestamp,
    pub section: Option<TaskSection>,
    pub blocked_by: Option<BlockedBy>,
    pub effort: Option<EffortTier>,
    pub priority: Option<PriorityTier>,
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
    pub completed_at: NullablePatch<TaskTimestamp>,
    pub commits: NullablePatch<String>,
    pub body: Option<String>,
    pub title: Option<TaskTitle>,
    pub blocked_by: NullablePatch<BlockedBy>,
    pub effort: NullablePatch<EffortTier>,
    pub priority: NullablePatch<PriorityTier>,
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

/// Records whether an index entry is open or completed at a known instant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexEntryState {
    Open,
    Done(Option<TaskTimestamp>),
}

/// Binds one task identity to the persisted revision used by an operation's read phase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedTaskRevision {
    pub id: TaskId,
    pub revision: ContentRevision,
}

/// Describes one task-specific persistence change without exposing filesystem mechanics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskWrite {
    Patch {
        id: TaskId,
        patch: TaskPatch,
    },
    MoveToTrash {
        id: TaskId,
    },
    UpsertIndex(IndexEntry),
    DeleteIndex(TaskId),
    RenameIndexSection {
        current_label: TaskSection,
        new_label: TaskSection,
    },
}

impl TaskWrite {
    fn task_id(&self) -> Option<&TaskId> {
        match self {
            Self::Patch { id, .. } | Self::MoveToTrash { id } | Self::DeleteIndex(id) => Some(id),
            Self::UpsertIndex(entry) => Some(&entry.id),
            Self::RenameIndexSection { .. } => None,
        }
    }
}

/// Carries a complete guarded task mutation across the application/store boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskWriteSet {
    expected: Vec<ExpectedTaskRevision>,
    writes: Vec<TaskWrite>,
}

impl TaskWriteSet {
    pub fn try_new(
        expected: Vec<ExpectedTaskRevision>,
        writes: Vec<TaskWrite>,
    ) -> Result<Self, TaskWriteSetError> {
        if expected.is_empty() {
            return Err(TaskWriteSetError::EmptyExpectations);
        }
        let expected_ids = validate_expectations(&expected)?;
        let mut task_mutations = BTreeSet::new();
        let mut index_mutations = BTreeSet::new();
        for write in &writes {
            validate_write_expectation(write, &expected_ids)?;
            validate_unique_write(write, &mut task_mutations, &mut index_mutations)?;
        }
        Ok(Self { expected, writes })
    }

    #[must_use]
    pub fn expected(&self) -> &[ExpectedTaskRevision] {
        &self.expected
    }

    #[must_use]
    pub fn writes(&self) -> &[TaskWrite] {
        &self.writes
    }

    #[must_use]
    pub fn into_parts(self) -> (Vec<ExpectedTaskRevision>, Vec<TaskWrite>) {
        (self.expected, self.writes)
    }
}

fn validate_expectations(
    expected: &[ExpectedTaskRevision],
) -> Result<BTreeSet<TaskId>, TaskWriteSetError> {
    let mut ids = BTreeSet::new();
    for expectation in expected {
        register_expectation(&mut ids, expectation)?;
    }
    Ok(ids)
}

fn register_expectation(
    ids: &mut BTreeSet<TaskId>,
    expectation: &ExpectedTaskRevision,
) -> Result<(), TaskWriteSetError> {
    if ids.insert(expectation.id.clone()) {
        return Ok(());
    }
    Err(TaskWriteSetError::DuplicateExpectation {
        id: expectation.id.clone(),
    })
}

fn validate_write_expectation(
    write: &TaskWrite,
    expected_ids: &BTreeSet<TaskId>,
) -> Result<(), TaskWriteSetError> {
    let Some(id) = write.task_id() else {
        return Ok(());
    };
    if expected_ids.contains(id) {
        return Ok(());
    }
    Err(TaskWriteSetError::MissingExpectation { id: id.clone() })
}

fn validate_unique_write(
    write: &TaskWrite,
    task_mutations: &mut BTreeSet<TaskId>,
    index_mutations: &mut BTreeSet<TaskId>,
) -> Result<(), TaskWriteSetError> {
    let target = match write {
        TaskWrite::Patch { id, .. } | TaskWrite::MoveToTrash { id } => Some((task_mutations, id)),
        TaskWrite::UpsertIndex(entry) => Some((index_mutations, &entry.id)),
        TaskWrite::DeleteIndex(id) => Some((index_mutations, id)),
        TaskWrite::RenameIndexSection { .. } => None,
    };
    let Some((targets, id)) = target else {
        return Ok(());
    };
    if targets.insert(id.clone()) {
        return Ok(());
    }
    Err(TaskWriteSetError::ConflictingWrite { id: id.clone() })
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TaskWriteSetError {
    #[error("a task write set requires at least one expected revision")]
    EmptyExpectations,
    #[error("task {id} has more than one expected revision")]
    DuplicateExpectation { id: TaskId },
    #[error("task write for {id} has no expected revision")]
    MissingExpectation { id: TaskId },
    #[error("task {id} has contradictory writes in one write set")]
    ConflictingWrite { id: TaskId },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskRevisionState {
    Present(ContentRevision),
    Missing,
}

impl std::fmt::Display for TaskRevisionState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Present(revision) => revision.fmt(formatter),
            Self::Missing => formatter.write_str("<missing>"),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TaskMutationError<E> {
    #[error(
        "task {id} changed since it was read (expected revision {expected}, current revision {current})"
    )]
    StaleTask {
        id: TaskId,
        expected: ContentRevision,
        current: TaskRevisionState,
    },
    #[error("a task mutation source changed while the write was being prepared")]
    SourceChanged,
    #[error("task mutation store failed: {0}")]
    Store(E),
}

impl<E> TaskMutationError<E> {
    pub fn map_store<F>(self, map: impl FnOnce(E) -> F) -> TaskMutationError<F> {
        match self {
            Self::StaleTask {
                id,
                expected,
                current,
            } => TaskMutationError::StaleTask {
                id,
                expected,
                current,
            },
            Self::SourceChanged => TaskMutationError::SourceChanged,
            Self::Store(source) => TaskMutationError::Store(map(source)),
        }
    }
}

/// Persists the task records and project-index state used by task interactors.
pub trait TaskVault: Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;

    fn get_task(&self, project: &Project, id: &TaskId) -> Result<Option<TaskRecord>, Self::Error>;
    fn list_tasks(&self, project: &Project) -> Result<Vec<TaskRecord>, Self::Error>;
    fn next_task_id(&self, project: &Project) -> Result<TaskId, Self::Error>;
    fn insert_task(
        &self,
        project: &Project,
        id: &TaskId,
        new: NewTask,
    ) -> Result<TaskRecord, Self::Error>;
    fn read_task_markdown(&self, locator: &TaskNotePath) -> Result<String, Self::Error>;
    fn list_index_entries(&self, project: &Project) -> Result<Vec<IndexEntry>, Self::Error>;
    fn list_index_sections(&self, project: &Project) -> Result<Vec<TaskSection>, Self::Error>;
    fn upsert_index_entry(&self, project: &Project, entry: IndexEntry) -> Result<(), Self::Error>;
    fn commit_task_writes(
        &self,
        project: &Project,
        writes: TaskWriteSet,
    ) -> Result<(), TaskMutationError<Self::Error>>;
}

#[cfg(test)]
mod tests {
    use pwf_models::{revision::ContentRevision, task::TaskId};

    use super::{ExpectedTaskRevision, TaskPatch, TaskWrite, TaskWriteSet, TaskWriteSetError};

    fn id(value: &str) -> TaskId {
        TaskId::try_new(value).unwrap()
    }

    fn expected(value: &str) -> ExpectedTaskRevision {
        ExpectedTaskRevision {
            id: id(value),
            revision: ContentRevision::try_new("0".repeat(64)).unwrap(),
        }
    }

    #[test]
    fn write_set_rejects_a_write_without_an_expectation() {
        let error = TaskWriteSet::try_new(
            vec![expected("FOO-0001")],
            vec![TaskWrite::Patch {
                id: id("FOO-0002"),
                patch: TaskPatch::default(),
            }],
        )
        .unwrap_err();

        assert_eq!(
            error,
            TaskWriteSetError::MissingExpectation { id: id("FOO-0002") }
        );
    }

    #[test]
    fn write_set_rejects_two_task_mutations_for_one_id() {
        let error = TaskWriteSet::try_new(
            vec![expected("FOO-0001")],
            vec![
                TaskWrite::Patch {
                    id: id("FOO-0001"),
                    patch: TaskPatch::default(),
                },
                TaskWrite::MoveToTrash { id: id("FOO-0001") },
            ],
        )
        .unwrap_err();

        assert_eq!(
            error,
            TaskWriteSetError::ConflictingWrite { id: id("FOO-0001") }
        );
    }

    #[test]
    fn expectation_only_write_set_is_valid() {
        let writes = TaskWriteSet::try_new(vec![expected("FOO-0001")], Vec::new()).unwrap();

        assert!(writes.writes().is_empty());
    }
}
