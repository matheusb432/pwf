use std::{
    fmt,
    path::{Path, PathBuf},
};

use pwf_models::{
    project::ProjectId,
    revision::ContentRevision,
    task::{
        BlockedBy, CommitRanges, EffortTier, PriorityTier, TaskBody, TaskId, TaskReport,
        TaskStatus, TaskTags, TaskTitle,
    },
};

use crate::{collection_edit::CollectionEdit, patch_field::PatchField, set_field::SetField};

mod body_sections;
pub use body_sections::{
    GetTaskBodySections, TaskBodyItemStyle, TaskBodySection, TaskBodySections,
};
mod dag;
pub use dag::{
    GetTaskDag, TaskDag, TaskDagDepth, TaskDagDepthError, TaskDagEdge, TaskDagError, TaskDagMode,
    TaskDagNode,
};
mod list;
pub mod session;
pub use list::{
    BlockedByIssue, BlockedByResolution, BlockedByStatus, ListDetail, ListTasks, ListedTask,
    ListedTaskDetails, ListedTasks, StatusFilter, TaskHeading, TaskIssue, TaskLaunch, TaskPageSize,
    TaskPageSizeError, TaskPageToken, TaskPageTokenError,
};
mod record;
pub use pwf_models::task::{Task, TaskListLimit, TaskListLimitError};
pub use record::{RawTaskTags, StoredBlockedBy, TaskRecord, TaskRecordError};

#[derive(Debug, PartialEq, Eq)]
pub enum AddTaskBody {
    Shorthand(String),
    Body { title: TaskTitle, body: TaskBody },
}

impl AddTaskBody {
    #[must_use]
    pub fn from_shorthand(raw: impl Into<String>) -> Self {
        let mut raw = raw.into();
        let start = raw.len() - raw.trim_start().len();
        raw.truncate(start + raw.trim().len());
        raw.drain(..start);
        Self::Shorthand(raw)
    }

    #[must_use]
    pub fn from_body(title: TaskTitle, body: TaskBody) -> Self {
        Self::Body { title, body }
    }
}

/// Requests creation of one task.
#[derive(Debug)]
pub struct AddTask {
    /// Destination project ID.
    pub project_id: ProjectId,
    /// Shorthand or verbatim task body.
    pub body: AddTaskBody,
    /// Task IDs in the `blocked_by` relationship.
    pub blocked_by: Option<BlockedBy>,
    /// Optional effort tier.
    pub effort: Option<EffortTier>,
    /// Optional normalized discovery tags.
    pub tags: Option<TaskTags>,
    /// Optional scheduling priority.
    pub priority: Option<PriorityTier>,
}

/// Requests creation of one task from an authored Markdown file.
#[derive(Debug)]
pub struct AddTaskFromFile {
    pub project_id: ProjectId,
    /// Local source file whose contents become the task body.
    pub source_file: PathBuf,
    /// Omission derives the title from the source filename stem.
    pub title: Option<TaskTitle>,
}

/// Copies task content and metadata into a new active task.
#[derive(Debug, Clone)]
pub struct CloneTask {
    /// Source task ID.
    pub id: TaskId,
    /// Destination project, defaulting to the source task's project.
    pub project_id: ClonedTaskProjectId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClonedTaskProjectId {
    SameAsTask,
    Id(ProjectId),
}
impl ClonedTaskProjectId {
    #[must_use]
    pub fn new(value: impl Into<Option<ProjectId>>) -> Self {
        value.into().map_or(Self::SameAsTask, Self::Id)
    }
}

impl AddTask {
    #[must_use]
    pub fn new(project_id: impl Into<ProjectId>, body: AddTaskBody) -> Self {
        Self {
            project_id: project_id.into(),
            body,
            blocked_by: None,
            effort: None,
            tags: None,
            priority: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CancelTask {
    pub id: TaskId,
    pub report: TaskReport,
    pub commits: Option<CommitRanges>,
    pub expected_revision: Option<ContentRevision>,
}

#[derive(Debug, Clone)]
pub struct CompleteTask {
    pub id: TaskId,
    pub report: Option<TaskReport>,
    pub commits: Option<CommitRanges>,
    pub expected_revision: Option<ContentRevision>,
}

#[derive(Debug, Clone)]
pub enum EditTaskContentKind {
    Title(TaskTitle),
    ReplaceBody {
        title: SetField<TaskTitle>,
        body: TaskBody,
    },
    AppendShorthand {
        title: SetField<TaskTitle>,
        body: String,
    },
    ReplaceShorthand {
        body: String,
    },
}

/// Carries one structurally valid task-content change.
#[derive(Debug, Clone)]
pub struct EditTaskContent(EditTaskContentKind);

impl EditTaskContent {
    /// Creates a title replacement that leaves the body unchanged.
    #[must_use]
    pub fn title(title: TaskTitle) -> Self {
        Self(EditTaskContentKind::Title(title))
    }

    /// Replaces the authored body without parsing shorthand; omission keeps the title.
    #[must_use]
    pub fn replace_body(title: SetField<TaskTitle>, body: TaskBody) -> Self {
        Self(EditTaskContentKind::ReplaceBody { title, body })
    }

    /// Creates a non-empty shorthand append.
    pub fn append_shorthand(
        title: SetField<TaskTitle>,
        body: String,
    ) -> Result<Self, EditTaskContentError> {
        if body.trim().is_empty() {
            return Err(EditTaskContentError::EmptyAppend);
        }
        Ok(Self(EditTaskContentKind::AppendShorthand { title, body }))
    }

    /// Creates a shorthand replacement for application parsing with the runtime section syntax.
    #[must_use]
    pub fn replace_shorthand(body: String) -> Self {
        Self(EditTaskContentKind::ReplaceShorthand { body })
    }

    #[must_use]
    pub fn kind(&self) -> &EditTaskContentKind {
        &self.0
    }
}

/// Reports an invalid task-content edit before application execution.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EditTaskContentError {
    #[error("--append-body cannot be empty.")]
    EmptyAppend,
}

/// Carries at least one requested task change.
#[derive(Debug, Clone)]
pub struct TaskEdits {
    content: SetField<EditTaskContent>,
    blocked_by: CollectionEdit<BlockedBy>,
    effort: PatchField<EffortTier>,
    tags: CollectionEdit<TaskTags>,
    priority: PatchField<PriorityTier>,
}

impl TaskEdits {
    /// Creates a non-empty set of task changes.
    ///
    /// # Errors
    ///
    /// Returns [`EmptyTaskEdits`] when every field is unchanged.
    pub fn try_new(
        content: SetField<EditTaskContent>,
        blocked_by: CollectionEdit<BlockedBy>,
        effort: PatchField<EffortTier>,
        tags: CollectionEdit<TaskTags>,
        priority: PatchField<PriorityTier>,
    ) -> Result<Self, EmptyTaskEdits> {
        if content.is_unchanged()
            && blocked_by.is_unchanged()
            && effort.is_unchanged()
            && tags.is_unchanged()
            && priority.is_unchanged()
        {
            return Err(EmptyTaskEdits);
        }
        Ok(Self {
            content,
            blocked_by,
            effort,
            tags,
            priority,
        })
    }

    #[must_use]
    pub fn content(&self) -> &SetField<EditTaskContent> {
        &self.content
    }

    #[must_use]
    pub fn blocked_by(&self) -> &CollectionEdit<BlockedBy> {
        &self.blocked_by
    }

    #[must_use]
    pub fn effort(&self) -> &PatchField<EffortTier> {
        &self.effort
    }

    #[must_use]
    pub fn tags(&self) -> &CollectionEdit<TaskTags> {
        &self.tags
    }

    #[must_use]
    pub fn priority(&self) -> &PatchField<PriorityTier> {
        &self.priority
    }
}

/// Reports an edit request with no changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("nothing to edit; pass at least one edit flag.")]
pub struct EmptyTaskEdits;

#[derive(Debug, Clone)]
pub struct EditTask {
    pub id: TaskId,
    pub edits: TaskEdits,
    pub expected_revision: Option<ContentRevision>,
}

/// Requests one confirmed task deletion.
#[derive(Debug, Clone)]
pub struct DeleteTask {
    pub id: TaskId,
}

/// Requests activation, confirming removal of completion data only for closed tasks.
#[derive(Debug, Clone)]
pub struct ActivateTask {
    pub id: TaskId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BacklogTaskOutcome {
    Backlogged,
    AlreadyBacklogged,
}

/// Identifies a task file's filesystem path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskFilePath(PathBuf);

impl TaskFilePath {
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self(path)
    }

    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }

    #[must_use]
    pub fn into_path_buf(self) -> PathBuf {
        self.0
    }

    #[must_use]
    pub fn into_display_string(self) -> String {
        self.0
            .into_os_string()
            .into_string()
            .unwrap_or_else(|path| path.to_string_lossy().into_owned())
    }
}

impl fmt::Display for TaskFilePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0.display())
    }
}

/// Identifies one managed project's task-root path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectTaskPath(PathBuf);

impl ProjectTaskPath {
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self(path)
    }

    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }

    #[must_use]
    pub fn into_path_buf(self) -> PathBuf {
        self.0
    }
}

impl fmt::Display for ProjectTaskPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0.display())
    }
}

/// Selects the lifecycle transition performed while closing a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClosedTaskAction {
    Done,
    Cancelled,
}

impl ClosedTaskAction {
    #[must_use]
    pub const fn past_tense(self) -> &'static str {
        match self {
            Self::Done => "Done",
            Self::Cancelled => "Cancelled",
        }
    }
}

/// Captures the task fields returned by a committed mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskMutationSummary {
    pub id: TaskId,
    pub title: String,
    pub status: TaskStatus,
}

/// Aborted confirmations return no task summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskMutationResult<T> {
    pub outcome: T,
    pub task: Option<TaskMutationSummary>,
}

/// Reports whether a confirmed task deletion proceeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteTaskOutcome {
    Deleted,
    Aborted,
}

/// Reports the result of task activation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivateTaskOutcome {
    Activated,
    AlreadyActive,
    Aborted,
}

#[cfg(test)]
mod tests {
    use super::{AddTaskBody, EditTaskContent, EditTaskContentError, EmptyTaskEdits, TaskEdits};
    use crate::{collection_edit::CollectionEdit, patch_field::PatchField, set_field::SetField};

    #[test]
    fn shorthand_trims_outer_whitespace_and_accepts_empty_input() {
        for (raw, expected) in [
            (" \n\t ", ""),
            ("  title only  ", "title only"),
            (
                "\u{2003}título /g keep  spacing\n  ",
                "título /g keep  spacing",
            ),
        ] {
            let body: AddTaskBody = AddTaskBody::from_shorthand(raw);
            assert!(matches!(body, super::AddTaskBody::Shorthand(body) if body == expected));
        }
    }

    #[test]
    fn add_body_variants_preserve_authored_content() {
        let body = AddTaskBody::from_shorthand("task /g keep text");
        assert!(matches!(body, super::AddTaskBody::Shorthand(body) if body == "task /g keep text"));
        let title = pwf_models::task::TaskTitle::try_new("typed task").unwrap();
        let body = AddTaskBody::from_body(title, pwf_models::task::TaskBody::new("## Goals"));
        assert!(
            matches!(body, super::AddTaskBody::Body { title, body } if title.as_ref() == "typed task" && body.as_ref() == "## Goals")
        );
    }

    #[test]
    fn edit_contracts_reject_empty_shapes() {
        assert!(matches!(
            EditTaskContent::append_shorthand(SetField::NoAction, " \n\t ".into()),
            Err(EditTaskContentError::EmptyAppend)
        ));
        let error = TaskEdits::try_new(
            SetField::NoAction,
            CollectionEdit::Unchanged,
            PatchField::NoAction,
            CollectionEdit::Unchanged,
            PatchField::NoAction,
        )
        .unwrap_err();
        assert_eq!(error, EmptyTaskEdits);
    }
}
