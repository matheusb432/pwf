use std::collections::BTreeSet;

use pwf_models::{
    project::Project,
    revision::ContentRevision,
    task::{
        BlockedBy, EffortTier, PriorityTier, TaskId, TaskPrompt, TaskStatus, TaskTags,
        TaskTimestamp, TaskTitle,
    },
};
use pwf_wire::{
    set_field::SetField,
    task::{RawTaskTags, StoredBlockedBy, TaskNotePath, TaskRecord},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSummaryRecord {
    pub id: TaskId,
    pub title: String,
    pub status: TaskStatus,
    pub created_at: Option<TaskTimestamp>,
    pub tags: Option<RawTaskTags>,
    pub effort: Option<String>,
    pub priority: Option<String>,
}

impl From<TaskRecord> for TaskSummaryRecord {
    fn from(record: TaskRecord) -> Self {
        Self {
            id: record.id,
            title: record.title,
            status: record.status,
            created_at: record.created_at,
            tags: record.tags,
            effort: record.effort,
            priority: record.priority,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskDependencyRecord {
    pub blocked_by: StoredBlockedBy,
    pub locator: TaskNotePath,
}

/// Rendered content receives note framing; verbatim bodies retain every byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NewTaskBody {
    Rendered(String),
    Verbatim(TaskPrompt),
}

impl AsRef<str> for NewTaskBody {
    fn as_ref(&self) -> &str {
        match self {
            Self::Rendered(body) => body,
            Self::Verbatim(body) => body.as_ref(),
        }
    }
}

impl From<String> for NewTaskBody {
    fn from(body: String) -> Self {
        Self::Rendered(body)
    }
}

impl From<NewTaskBody> for String {
    fn from(body: NewTaskBody) -> Self {
        match body {
            NewTaskBody::Rendered(body) => body,
            NewTaskBody::Verbatim(body) => body.into_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTask {
    pub body: NewTaskBody,
    pub title: TaskTitle,
    pub created_at: TaskTimestamp,
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
    pub status: SetField<TaskStatus>,
    pub completed_at: NullablePatch<TaskTimestamp>,
    pub commits: NullablePatch<String>,
    pub body: SetField<String>,
    pub title: SetField<TaskTitle>,
    pub blocked_by: NullablePatch<BlockedBy>,
    pub effort: NullablePatch<EffortTier>,
    pub priority: NullablePatch<PriorityTier>,
    pub tags: NullablePatch<TaskTags>,
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
    DeleteNote {
        id: TaskId,
        deletion: pwf_wire::confirmation::TaskDeletion,
    },
}

impl TaskWrite {
    fn task_id(&self) -> &TaskId {
        match self {
            Self::Patch { id, .. } | Self::DeleteNote { id, .. } => id,
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
        for write in &writes {
            validate_write_expectation(write, &expected_ids)?;
            validate_unique_write(write, &mut task_mutations)?;
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
    let id = write.task_id();
    if expected_ids.contains(id) {
        return Ok(());
    }
    Err(TaskWriteSetError::MissingExpectation { id: id.clone() })
}

fn validate_unique_write(
    write: &TaskWrite,
    task_mutations: &mut BTreeSet<TaskId>,
) -> Result<(), TaskWriteSetError> {
    let id = write.task_id();
    if task_mutations.insert(id.clone()) {
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

/// Persists task notes and applies writes guarded by their revisions.
pub trait TaskVault: Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Resolves the configured deletion destination and checks that its trash folder exists.
    fn task_deletion(
        &self,
        project: &Project,
    ) -> Result<pwf_wire::confirmation::TaskDeletion, Self::Error>;

    fn get_task_record(
        &self,
        project: &Project,
        id: &TaskId,
    ) -> Result<Option<TaskRecord>, Self::Error>;
    /// Reads dependency metadata without loading the task body.
    fn get_task_dependencies(
        &self,
        project: &Project,
        id: &TaskId,
    ) -> Result<Option<TaskDependencyRecord>, Self::Error>;

    fn list_tasks(&self, project: &Project) -> Result<Vec<TaskRecord>, Self::Error>;

    fn list_task_summaries(
        &self,
        project: &Project,
    ) -> Result<Vec<TaskSummaryRecord>, Self::Error> {
        self.list_tasks(project)
            .map(|records| records.into_iter().map(TaskSummaryRecord::from).collect())
    }
    fn next_task_id(&self, project: &Project) -> Result<TaskId, Self::Error>;
    fn insert_task(
        &self,
        project: &Project,
        id: &TaskId,
        new: NewTask,
    ) -> Result<TaskRecord, Self::Error>;
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
                TaskWrite::DeleteNote {
                    id: id("FOO-0001"),
                    deletion: pwf_wire::confirmation::TaskDeletion::HardDelete,
                },
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
