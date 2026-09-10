use std::{
    fmt,
    path::{Path, PathBuf},
};

use pwf_models::{
    project::ProjectId,
    revision::ContentRevision,
    task::{
        BlockedBy, CommitRanges, EffortTier, PriorityTier, TaskId, TaskPrompt, TaskReport,
        TaskStatus, TaskTags, TaskTitle,
    },
};

use crate::{collection_edit::CollectionEdit, patch_field::PatchField, set_field::SetField};

mod dag;
pub use dag::{
    GetTaskDag, TaskDag, TaskDagDepth, TaskDagDepthError, TaskDagEdge, TaskDagError, TaskDagMode,
    TaskDagNode,
};
mod list;
pub mod session;
pub use list::{
    BlockedByIssue, BlockedByResolution, BlockedByStatus, ListDetail, ListLayout, ListScope,
    ListTasks, ListedTask, ListedTaskDetails, ListedTasks, StatusFilter, TaskHeading, TaskIssue,
    TaskLaunch, TaskListLimit, TaskListLimitError, TaskLocation, TaskPageSize, TaskPageSizeError,
    TaskPageToken, TaskPageTokenError,
};
mod record;
pub use pwf_models::task::Task;
pub use record::{
    IndexPlacement, Materialization, RawTaskTags, StoredBlockedBy, TaskRecord, TaskRecordError,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskLane {
    Goal,
    Context,
    Constraint,
    DoneWhen,
}

impl TaskLane {
    fn label(self) -> &'static str {
        match self {
            Self::Goal => "goal",
            Self::Context => "context",
            Self::Constraint => "constraint",
            Self::DoneWhen => "done when",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TaskLaneValueError {
    #[error("{} cannot be empty.", lane.label())]
    Empty { lane: TaskLane },
    #[error("{} must be a single line.", lane.label())]
    Multiline { lane: TaskLane },
}

impl TaskLaneValueError {
    #[must_use]
    pub fn lane(&self) -> TaskLane {
        match self {
            Self::Empty { lane } | Self::Multiline { lane } => *lane,
        }
    }

    #[must_use]
    pub fn reason(&self) -> &'static str {
        match self {
            Self::Empty { .. } => "cannot be empty.",
            Self::Multiline { .. } => "must be a single line.",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskLanes {
    goals: Vec<String>,
    context: Vec<String>,
    constraints: Vec<String>,
    done_when: Vec<String>,
}

impl TaskLanes {
    /// Constructs ordered task lanes after trimming each single-line value.
    ///
    /// # Errors
    ///
    /// Returns [`TaskLaneValueError`] when any value is blank or contains a line break.
    pub fn try_new(
        goals: Vec<String>,
        context: Vec<String>,
        constraints: Vec<String>,
        done_when: Vec<String>,
    ) -> Result<Self, TaskLaneValueError> {
        Ok(Self {
            goals: normalize_lane(TaskLane::Goal, goals)?,
            context: normalize_lane(TaskLane::Context, context)?,
            constraints: normalize_lane(TaskLane::Constraint, constraints)?,
            done_when: normalize_lane(TaskLane::DoneWhen, done_when)?,
        })
    }

    #[must_use]
    pub fn goals(&self) -> &[String] {
        &self.goals
    }

    #[must_use]
    pub fn context(&self) -> &[String] {
        &self.context
    }

    #[must_use]
    pub fn constraints(&self) -> &[String] {
        &self.constraints
    }

    #[must_use]
    pub fn done_when(&self) -> &[String] {
        &self.done_when
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.goals.is_empty()
            && self.context.is_empty()
            && self.constraints.is_empty()
            && self.done_when.is_empty()
    }
}

fn normalize_lane(lane: TaskLane, values: Vec<String>) -> Result<Vec<String>, TaskLaneValueError> {
    values
        .into_iter()
        .map(|value| {
            if value.contains(['\n', '\r']) {
                return Err(TaskLaneValueError::Multiline { lane });
            }
            let value = value.trim().to_string();
            if value.is_empty() {
                return Err(TaskLaneValueError::Empty { lane });
            }
            Ok(value)
        })
        .collect()
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskLaneEdits {
    additions: TaskLanes,
    removals: Vec<TaskLane>,
}

impl TaskLaneEdits {
    #[must_use]
    pub fn new(additions: TaskLanes, removals: impl IntoIterator<Item = TaskLane>) -> Self {
        let mut normalized_removals = Vec::new();
        for lane in removals {
            push_unseen_lane(&mut normalized_removals, lane);
        }
        Self {
            additions,
            removals: normalized_removals,
        }
    }

    #[must_use]
    pub fn additions(&self) -> &TaskLanes {
        &self.additions
    }

    #[must_use]
    pub fn removals(&self) -> &[TaskLane] {
        &self.removals
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.additions.is_empty() && self.removals.is_empty()
    }
}

fn push_unseen_lane(lanes: &mut Vec<TaskLane>, lane: TaskLane) {
    if !lanes.contains(&lane) {
        lanes.push(lane);
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum AddTaskPrompt {
    Shorthand(String),
    Structured { title: TaskTitle, lanes: TaskLanes },
    Body { title: TaskTitle, body: TaskPrompt },
}

impl AddTaskPrompt {
    #[must_use]
    pub fn from_shorthand(raw: impl Into<String>) -> Self {
        let mut raw = raw.into();
        let start = raw.len() - raw.trim_start().len();
        raw.truncate(start + raw.trim().len());
        raw.drain(..start);
        Self::Shorthand(raw)
    }

    #[must_use]
    pub fn from_structured(title: TaskTitle, lanes: TaskLanes) -> Self {
        Self::Structured { title, lanes }
    }

    #[must_use]
    pub fn from_body(title: TaskTitle, body: TaskPrompt) -> Self {
        Self::Body { title, body }
    }
}

/// Requests creation of one task.
#[derive(Debug)]
pub struct AddTask {
    /// Destination project ID.
    pub project_id: ProjectId,
    /// Shorthand or structured task prompt.
    pub prompt: AddTaskPrompt,
    /// Task IDs in the `blocked_by` relationship.
    pub blocked_by: Option<BlockedBy>,
    /// Optional effort tier.
    pub effort: Option<EffortTier>,
    /// Optional normalized discovery tags.
    pub tags: Option<TaskTags>,
    /// Optional scheduling priority.
    pub priority: Option<PriorityTier>,
    /// Retry identity supplied by API clients that can replay creation.
    pub request_id: Option<TaskRequestId>,
    /// Stable fingerprint of the validated transport request without its retry identity.
    pub request_fingerprint: Option<TaskRequestFingerprint>,
}

/// Copies task content and metadata into a new active task.
#[derive(Debug, Clone)]
pub struct CloneTask {
    /// Source task ID.
    pub id: TaskId,
    /// Destination project, defaulting to the source task's project.
    pub project_id: ClonedTaskProjectId,
    pub request_id: Option<TaskRequestId>,
    pub request_fingerprint: Option<TaskRequestFingerprint>,
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
    pub fn new(project_id: impl Into<ProjectId>, prompt: AddTaskPrompt) -> Self {
        Self {
            project_id: project_id.into(),
            prompt,
            blocked_by: None,
            effort: None,
            tags: None,
            priority: None,
            request_id: None,
            request_fingerprint: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CancelTask {
    pub id: TaskId,
    pub report: TaskReport,
    pub commits: Option<CommitRanges>,
    pub expected_revision: Option<ContentRevision>,
    pub request_id: Option<TaskRequestId>,
    pub request_fingerprint: Option<TaskRequestFingerprint>,
}

#[derive(Debug, Clone)]
pub struct CompleteTask {
    pub id: TaskId,
    pub report: Option<TaskReport>,
    pub commits: Option<CommitRanges>,
    pub expected_revision: Option<ContentRevision>,
    pub request_id: Option<TaskRequestId>,
    pub request_fingerprint: Option<TaskRequestFingerprint>,
}

#[derive(Debug, Clone)]
pub enum EditTaskContentKind {
    Structured {
        title: SetField<TaskTitle>,
        lanes: TaskLaneEdits,
    },
    AppendShorthand {
        title: SetField<TaskTitle>,
        prompt: String,
    },
    ReplaceShorthand {
        prompt: String,
    },
}

/// Carries one structurally valid task-content change.
#[derive(Debug, Clone)]
pub struct EditTaskContent(EditTaskContentKind);

impl EditTaskContent {
    /// Creates an explicit title or lane edit.
    pub fn structured(
        title: SetField<TaskTitle>,
        lanes: TaskLaneEdits,
    ) -> Result<Self, EditTaskContentError> {
        if title.is_unchanged() && lanes.is_empty() {
            return Err(EditTaskContentError::EmptyStructured);
        }
        Ok(Self(EditTaskContentKind::Structured { title, lanes }))
    }

    /// Creates a non-empty shorthand append.
    pub fn append_shorthand(
        title: SetField<TaskTitle>,
        prompt: String,
    ) -> Result<Self, EditTaskContentError> {
        if prompt.trim().is_empty() {
            return Err(EditTaskContentError::EmptyAppend);
        }
        Ok(Self(EditTaskContentKind::AppendShorthand { title, prompt }))
    }

    /// Creates a shorthand replacement for application parsing with the runtime lane syntax.
    #[must_use]
    pub fn replace_shorthand(prompt: String) -> Self {
        Self(EditTaskContentKind::ReplaceShorthand { prompt })
    }

    #[must_use]
    pub fn kind(&self) -> &EditTaskContentKind {
        &self.0
    }
}

/// Reports an invalid task-content edit before application execution.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EditTaskContentError {
    #[error("task content edit cannot be empty")]
    EmptyStructured,
    #[error("--append cannot be empty.")]
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
    pub request_id: Option<TaskRequestId>,
    pub request_fingerprint: Option<TaskRequestFingerprint>,
}

/// Requests one replayable confirmed task deletion.
#[derive(Debug, Clone)]
pub struct DeleteTask {
    pub id: TaskId,
    pub request_id: Option<TaskRequestId>,
    pub request_fingerprint: Option<TaskRequestFingerprint>,
}

/// Requests one replayable confirmed task reopen.
#[derive(Debug, Clone)]
pub struct ReopenTask {
    pub id: TaskId,
    pub request_id: Option<TaskRequestId>,
    pub request_fingerprint: Option<TaskRequestFingerprint>,
}

/// Identifies one replayable task mutation request.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TaskRequestId(Box<str>);

impl TaskRequestId {
    pub const MAX_LEN: usize = 64;

    /// Validates a bounded request identifier safe for durable storage.
    pub fn try_new(value: impl Into<String>) -> Result<Self, TaskRequestIdError> {
        let value = value.into();
        if value.is_empty() || value.len() > Self::MAX_LEN {
            return Err(TaskRequestIdError::Length);
        }
        if !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err(TaskRequestIdError::Character);
        }
        Ok(Self(value.into_boxed_str()))
    }
}

impl AsRef<str> for TaskRequestId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TaskRequestId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TaskRequestIdError {
    #[error("must contain between 1 and {} characters", TaskRequestId::MAX_LEN)]
    Length,
    #[error("contains an unsupported character")]
    Character,
}

/// Identifies the exact mutation input bound to one request ID.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TaskRequestFingerprint(Box<str>);

impl TaskRequestFingerprint {
    #[must_use]
    pub fn from_digest(digest: [u8; 32]) -> Self {
        Self(digest_hex(digest).into_boxed_str())
    }
}

impl AsRef<str> for TaskRequestFingerprint {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TaskRequestFingerprint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

fn digest_hex(digest: [u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(64);
    for byte in digest {
        value.push(char::from(HEX[usize::from(byte >> 4)]));
        value.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    value
}

/// Identifies a task note's filesystem path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskNotePath(PathBuf);

impl TaskNotePath {
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

impl fmt::Display for TaskNotePath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0.display())
    }
}

/// Identifies a task index's filesystem path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskIndexPath(PathBuf);

impl TaskIndexPath {
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

impl fmt::Display for TaskIndexPath {
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

/// Older durable receipts retain their result without a task summary.
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

/// Reports the state reached by a task-reopening request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReopenTaskOutcome {
    Reopened,
    AlreadyActive,
    Aborted,
}

#[cfg(test)]
mod tests {
    use super::{
        AddTaskPrompt, EditTaskContent, EditTaskContentError, EmptyTaskEdits, TaskEdits,
        TaskLaneEdits, TaskLaneValueError, TaskLanes,
    };
    use crate::{collection_edit::CollectionEdit, patch_field::PatchField, set_field::SetField};

    #[test]
    fn lanes_trim_outer_whitespace_and_preserve_literal_markers() {
        let lanes = TaskLanes::try_new(
            vec!["  keep /c literal  ".to_string()],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .unwrap();

        assert_eq!(lanes.goals(), ["keep /c literal"]);
    }

    #[test]
    fn lanes_reject_blank_and_multiline_values() {
        assert!(matches!(
            TaskLanes::try_new(vec!["  ".to_string()], Vec::new(), Vec::new(), Vec::new()),
            Err(TaskLaneValueError::Empty { .. })
        ));
        assert!(matches!(
            TaskLanes::try_new(
                Vec::new(),
                vec!["one\ntwo".to_string()],
                Vec::new(),
                Vec::new(),
            ),
            Err(TaskLaneValueError::Multiline { .. })
        ));
    }

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
            let prompt: AddTaskPrompt = AddTaskPrompt::from_shorthand(raw);
            assert!(
                matches!(prompt, super::AddTaskPrompt::Shorthand(prompt) if prompt == expected)
            );
        }
    }

    #[test]
    fn add_prompt_variants_preserve_authored_content() {
        let prompt = AddTaskPrompt::from_shorthand("task /g keep text");
        assert!(
            matches!(prompt, super::AddTaskPrompt::Shorthand(prompt) if prompt == "task /g keep text")
        );
        let title = pwf_models::task::TaskTitle::try_new("typed task").unwrap();
        let prompt = AddTaskPrompt::from_structured(title, TaskLanes::default());
        assert!(
            matches!(prompt, super::AddTaskPrompt::Structured { title, lanes } if title.as_ref() == "typed task" && lanes.is_empty())
        );
    }

    #[test]
    fn edit_contracts_reject_empty_shapes() {
        assert!(matches!(
            EditTaskContent::structured(SetField::NoAction, TaskLaneEdits::default()),
            Err(EditTaskContentError::EmptyStructured)
        ));
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
