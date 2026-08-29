use std::{
    fmt,
    num::NonZeroUsize,
    path::{Path, PathBuf},
};

use pwf_models::{
    AppDate,
    project::{ProjectName, ProjectSelector, ProjectSourceValue},
    task::{
        BlockedBy, CommitRanges, EffortTier, IndexSection, PriorityTier, TaskId, TaskPrompt,
        TaskReport, TaskSection, TaskStatus, TaskTags, TaskTitle,
    },
};

pub mod session;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddTaskPromptKind {
    Shorthand(TaskPrompt),
    Structured { title: TaskTitle, lanes: TaskLanes },
}

/// Carries one structurally valid shorthand or structured add prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddTaskPrompt(AddTaskPromptKind);

impl AddTaskPrompt {
    /// Creates a non-empty shorthand prompt.
    pub fn shorthand(prompt: TaskPrompt) -> Result<Self, EmptyShorthandPrompt> {
        if prompt.as_ref().trim().is_empty() {
            return Err(EmptyShorthandPrompt);
        }
        Ok(Self(AddTaskPromptKind::Shorthand(prompt)))
    }

    #[must_use]
    pub fn structured(title: TaskTitle, lanes: TaskLanes) -> Self {
        Self(AddTaskPromptKind::Structured { title, lanes })
    }

    #[must_use]
    pub fn kind(&self) -> &AddTaskPromptKind {
        &self.0
    }
}

/// Reports a shorthand add prompt without authored content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("task shorthand prompt cannot be empty")]
pub struct EmptyShorthandPrompt;

/// Requests creation of one task.
#[derive(Debug, Clone)]
pub struct AddTask {
    /// Managed project name or project ID.
    pub project_selector: ProjectSelector,
    /// Shorthand or structured task prompt.
    pub prompt: AddTaskPrompt,
    /// Selects the task's index placement.
    pub index_section: IndexSection,
    /// Task IDs in the `blocked_by` relationship.
    pub blocked_by: Option<BlockedBy>,
    /// Optional effort tier.
    pub effort: Option<EffortTier>,
    /// Optional normalized discovery tags.
    pub tags: Option<TaskTags>,
    /// Optional scheduling priority.
    pub priority: Option<PriorityTier>,
}

#[derive(Debug, Clone)]
pub struct CancelTask {
    pub id: TaskId,
    pub report: TaskReport,
    pub commits: Option<CommitRanges>,
    pub review: bool,
}

#[derive(Debug, Clone)]
pub struct CompleteTask {
    pub id: TaskId,
    pub report: Option<TaskReport>,
    pub commits: Option<CommitRanges>,
    pub review: bool,
}

#[derive(Debug, Clone)]
pub enum EditTaskContentKind {
    Structured {
        title: Option<TaskTitle>,
        lanes: TaskLaneEdits,
    },
    AppendShorthand {
        title: Option<TaskTitle>,
        prompt: TaskPrompt,
    },
    ReplaceShorthand {
        prompt: TaskPrompt,
        title: TaskTitle,
    },
}

/// Carries one structurally valid task-content change.
#[derive(Debug, Clone)]
pub struct EditTaskContent(EditTaskContentKind);

impl EditTaskContent {
    /// Creates an explicit title or lane edit.
    pub fn structured(
        title: Option<TaskTitle>,
        lanes: TaskLaneEdits,
    ) -> Result<Self, EditTaskContentError> {
        if title.is_none() && lanes.is_empty() {
            return Err(EditTaskContentError::EmptyStructured);
        }
        Ok(Self(EditTaskContentKind::Structured { title, lanes }))
    }

    /// Creates a non-empty shorthand append.
    pub fn append_shorthand(
        title: Option<TaskTitle>,
        prompt: TaskPrompt,
    ) -> Result<Self, EditTaskContentError> {
        if prompt.as_ref().trim().is_empty() {
            return Err(EditTaskContentError::EmptyAppend);
        }
        Ok(Self(EditTaskContentKind::AppendShorthand { title, prompt }))
    }

    /// Creates a shorthand replacement with a validated leading title.
    pub fn replace_shorthand(prompt: TaskPrompt) -> Result<Self, EditTaskContentError> {
        let parsed = prompt_lanes::parse(prompt.as_ref());
        if parsed.title.trim().is_empty() {
            return Err(EditTaskContentError::MissingPromptTitle);
        }
        let title = TaskTitle::try_new(parsed.title).map_err(|error| {
            EditTaskContentError::InvalidTitle {
                message: error.to_string(),
            }
        })?;
        Ok(Self(EditTaskContentKind::ReplaceShorthand {
            prompt,
            title,
        }))
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
    #[error("--prompt must start with a nonempty title before any lane marker.")]
    MissingPromptTitle,
    #[error("{message}")]
    InvalidTitle { message: String },
}

/// Selects how an optional collection changes.
#[derive(Debug, Clone, Default)]
pub enum CollectionEdit<T> {
    /// Leaves the stored collection unchanged.
    #[default]
    Unchanged,
    /// Appends values to the stored collection.
    Append(T),
    /// Replaces the stored collection with the supplied values.
    Replace(T),
    /// Removes the stored collection.
    Clear,
}

impl<T> CollectionEdit<T> {
    #[must_use]
    pub fn addition(&self) -> Option<&T> {
        match self {
            Self::Append(value) | Self::Replace(value) => Some(value),
            Self::Unchanged | Self::Clear => None,
        }
    }

    fn is_unchanged(&self) -> bool {
        matches!(self, Self::Unchanged)
    }
}

/// Selects how an optional scalar value changes.
#[derive(Debug, Clone, Default)]
pub enum ValueEdit<T> {
    /// Leaves the stored value unchanged.
    #[default]
    Unchanged,
    /// Replaces the stored value.
    Set(T),
    /// Removes the stored value.
    Clear,
}

impl<T> ValueEdit<T> {
    fn is_unchanged(&self) -> bool {
        matches!(self, Self::Unchanged)
    }
}

/// Carries at least one requested task change.
#[derive(Debug, Clone)]
pub struct TaskEdits {
    content: Option<EditTaskContent>,
    blocked_by: CollectionEdit<BlockedBy>,
    effort: ValueEdit<EffortTier>,
    tags: CollectionEdit<TaskTags>,
    priority: ValueEdit<PriorityTier>,
}

impl TaskEdits {
    /// Creates a non-empty set of task changes.
    ///
    /// # Errors
    ///
    /// Returns [`EmptyTaskEdits`] when every field is unchanged.
    pub fn try_new(
        content: Option<EditTaskContent>,
        blocked_by: CollectionEdit<BlockedBy>,
        effort: ValueEdit<EffortTier>,
        tags: CollectionEdit<TaskTags>,
        priority: ValueEdit<PriorityTier>,
    ) -> Result<Self, EmptyTaskEdits> {
        if content.is_none()
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
    pub fn content(&self) -> Option<&EditTaskContent> {
        self.content.as_ref()
    }

    #[must_use]
    pub fn blocked_by(&self) -> &CollectionEdit<BlockedBy> {
        &self.blocked_by
    }

    #[must_use]
    pub fn effort(&self) -> &ValueEdit<EffortTier> {
        &self.effort
    }

    #[must_use]
    pub fn tags(&self) -> &CollectionEdit<TaskTags> {
        &self.tags
    }

    #[must_use]
    pub fn priority(&self) -> &ValueEdit<PriorityTier> {
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
}

/// Requests one task in a selected output representation.
#[derive(Debug, Clone)]
pub struct GetTask {
    /// Task ID.
    pub id: TaskId,
    /// Representation returned by the interactor.
    pub output: TaskReadFormat,
}

#[derive(Debug, Clone)]
pub struct ListTasks {
    pub project_selector: Option<ProjectSelector>,
    pub scope: ListScope,
    /// Explicit task cap. Omission uses the mode-specific default.
    pub number: Option<NonZeroUsize>,
    pub effort: Option<EffortTier>,
    pub priority: Option<PriorityTier>,
    pub tags: Option<TaskTags>,
    pub order: Option<OrderSpec>,
    /// Explicit lifecycle filter. Omission uses the mode-specific default.
    pub status: Option<StatusFilter>,
    pub detail: ListDetail,
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

/// Describes one newly persisted task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddedTask {
    pub id: TaskId,
    pub project: ProjectName,
    pub title: TaskTitle,
    pub note_path: TaskNotePath,
    pub created_section: Option<TaskSection>,
}

/// Carries task-add diagnostics that remain observable after a store failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddTaskDiagnostics {
    pub project: ProjectName,
    pub created_section: Option<TaskSection>,
}

/// Describes one task after an edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditedTask {
    pub id: TaskId,
    pub project: ProjectName,
    pub title: TaskTitle,
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

/// Describes one completed or cancelled task and its queue side effects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosedTask {
    pub id: TaskId,
    pub project: ProjectName,
    pub title: TaskTitle,
    pub action: ClosedTaskAction,
    pub evicted_ids: Vec<TaskId>,
    pub futuro_renamed_project: Option<ProjectName>,
    pub review_task: Option<AddedTask>,
}

/// Describes the result of a task-reopening request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReopenedTask {
    Reopened { id: TaskId, project: ProjectName },
    AlreadyActive { id: TaskId, project: ProjectName },
    Aborted { id: TaskId },
}

/// Describes the note and optional index link deleted with a task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovedTask {
    pub id: TaskId,
    pub project: ProjectName,
    pub title: TaskTitle,
    pub deleted_path: TaskNotePath,
    pub unlinked: Option<TaskIndexPath>,
}

/// Describes whether a confirmed task-removal request proceeded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemovedTaskOutcome {
    Removed(RemovedTask),
    Aborted { task_id: TaskId },
}

/// Selects the representation returned by a task read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskReadFormat {
    Markdown,
    Path,
    Data,
}

/// Contains one task's semantic data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskData {
    pub id: TaskId,
    pub project: ProjectName,
    pub title: TaskTitle,
    pub status: TaskStatus,
    pub created: Option<AppDate>,
    pub completed: Option<AppDate>,
    pub commits: Option<CommitRanges>,
    pub tags: Option<TaskTags>,
    pub effort: Option<EffortTier>,
    pub priority: Option<PriorityTier>,
    pub blocked_by: Option<BlockedBy>,
    pub section: Option<TaskSection>,
    pub prompt: TaskPrompt,
}

/// Carries one task in its requested representation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskRead {
    Markdown(String),
    Path(TaskNotePath),
    Data(Box<TaskData>),
}

/// Selects one lifecycle status or includes every lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusFilter {
    Exact(TaskStatus),
    All,
}

impl StatusFilter {
    #[must_use]
    pub fn includes(self, status: TaskStatus) -> bool {
        match self {
            Self::Exact(expected) => expected == status,
            Self::All => true,
        }
    }
}

impl Default for StatusFilter {
    fn default() -> Self {
        Self::Exact(TaskStatus::Active)
    }
}

/// Selects the task-index region included by a list request.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ListScope {
    #[default]
    Default,
    Human,
    Future,
    All,
}

/// Selects summary or metadata-rich task-list output.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ListDetail {
    #[default]
    Summary,
    Detailed,
}

impl ListDetail {
    #[must_use]
    pub const fn includes_relationship_statuses(self) -> bool {
        matches!(self, Self::Detailed)
    }
}

/// Selects the presentation implied by a resolved list scope.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ListLayout {
    #[default]
    Flat,
    BySection,
}

/// Selects the primary list ordering field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderField {
    Created,
    Id,
    ProjectId,
}

/// Selects ascending or descending list ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderDirection {
    Asc,
    Desc,
}

/// Combines the list ordering field and direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrderSpec {
    pub field: OrderField,
    pub direction: OrderDirection,
}

impl Default for OrderSpec {
    fn default() -> Self {
        Self {
            field: OrderField::Created,
            direction: OrderDirection::Desc,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockedByStatus {
    pub id: TaskId,
    pub title: Option<String>,
    pub resolution: BlockedByResolution,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockedByResolution {
    Found(TaskStatus),
    Missing,
    Unavailable { reason: String },
}

impl BlockedByResolution {
    #[must_use]
    pub fn is_warning(&self) -> bool {
        !matches!(self, Self::Found(TaskStatus::Done))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockedByIssue {
    Malformed {
        path: TaskNotePath,
        raw: String,
        reason: String,
    },
}

impl fmt::Display for BlockedByIssue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed { path, raw, reason } => write!(
                formatter,
                "Malformed blocked_by metadata {raw:?} in {path}: {reason}"
            ),
        }
    }
}

/// Preserves authored task-tags frontmatter until a use case requires validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawTaskTags(String);

impl RawTaskTags {
    #[must_use]
    pub fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }
}

impl AsRef<str> for RawTaskTags {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RawTaskTags {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Names a task with its authored title or its identifier fallback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskHeading {
    Title(TaskTitle),
    Identifier(TaskId),
}

impl AsRef<str> for TaskHeading {
    fn as_ref(&self) -> &str {
        match self {
            Self::Title(title) => title.as_ref(),
            Self::Identifier(id) => id.as_ref(),
        }
    }
}

impl fmt::Display for TaskHeading {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_ref())
    }
}

/// Describes one condition preventing task launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskIssue {
    MissingNote { path: TaskNotePath },
    PlaceholderPrompt,
}

impl fmt::Display for TaskIssue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingNote { path } => {
                write!(formatter, "Task note missing: {path}")
            }
            Self::PlaceholderPrompt => formatter
                .write_str("Prompt is a placeholder; define a real prompt before launching."),
        }
    }
}

/// Carries the derived launch state without duplicating readiness flags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskLaunch {
    issues: Vec<TaskIssue>,
}

impl TaskLaunch {
    #[must_use]
    pub fn from_issues(issues: Vec<TaskIssue>) -> Self {
        Self { issues }
    }

    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.issues.is_empty()
    }

    #[must_use]
    pub fn needs_prompt(&self) -> bool {
        self.issues.contains(&TaskIssue::PlaceholderPrompt)
    }

    #[must_use]
    pub fn issues(&self) -> &[TaskIssue] {
        &self.issues
    }
}

impl fmt::Display for TaskLaunch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut issues = self.issues.iter();
        if let Some(first) = issues.next() {
            write!(formatter, "{first}")?;
        }
        for issue in issues {
            write!(formatter, "; {issue}")?;
        }
        Ok(())
    }
}

/// Locates a task's index entry for display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskLocation {
    index_path: TaskIndexPath,
    line: NonZeroUsize,
}

impl TaskLocation {
    #[must_use]
    pub fn new(index_path: TaskIndexPath, line: NonZeroUsize) -> Self {
        Self { index_path, line }
    }

    #[must_use]
    pub fn index_path(&self) -> &TaskIndexPath {
        &self.index_path
    }

    #[must_use]
    pub fn line(&self) -> NonZeroUsize {
        self.line
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskView {
    pub id: TaskId,
    pub project: ProjectName,
    pub status: TaskStatus,
    pub heading: TaskHeading,
    pub prompt: TaskPrompt,
    pub project_path: ProjectSourceValue,
    pub location: TaskLocation,
    pub launch: TaskLaunch,
    pub section: Option<TaskSection>,
    pub blocked_by: Option<BlockedBy>,
    pub blocked_by_statuses: Vec<BlockedByStatus>,
    pub blocked_by_issues: Vec<BlockedByIssue>,
    pub effort: Option<EffortTier>,
    pub priority: Option<PriorityTier>,
    pub tags: Option<RawTaskTags>,
    pub created: Option<AppDate>,
}

/// Describes one resolved and capped task listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedTasks {
    pub tasks: Vec<TaskView>,
    pub hidden: usize,
    pub project: Option<ProjectName>,
    pub project_task_path: Option<ProjectTaskPath>,
    pub status_filter: StatusFilter,
    pub layout: ListLayout,
    pub detail: ListDetail,
}

#[cfg(test)]
mod tests {
    use pwf_models::task::TaskPrompt;

    use super::{
        AddTaskPrompt, CollectionEdit, EditTaskContent, EditTaskContentError, EmptyTaskEdits,
        TaskEdits, TaskLaneEdits, TaskLaneValueError, TaskLanes, ValueEdit,
    };

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
    fn add_prompt_rejects_blank_shorthand() {
        assert!(AddTaskPrompt::shorthand(TaskPrompt::new(" \n\t ")).is_err());
    }

    #[test]
    fn edit_contracts_reject_empty_shapes() {
        assert!(matches!(
            EditTaskContent::structured(None, TaskLaneEdits::default()),
            Err(EditTaskContentError::EmptyStructured)
        ));
        assert!(matches!(
            EditTaskContent::append_shorthand(None, TaskPrompt::new(" \n\t ")),
            Err(EditTaskContentError::EmptyAppend)
        ));
        assert!(matches!(
            EditTaskContent::replace_shorthand(TaskPrompt::new("/g replacement")),
            Err(EditTaskContentError::MissingPromptTitle)
        ));

        let error = TaskEdits::try_new(
            None,
            CollectionEdit::Unchanged,
            ValueEdit::Unchanged,
            CollectionEdit::Unchanged,
            ValueEdit::Unchanged,
        )
        .unwrap_err();
        assert_eq!(error, EmptyTaskEdits);
    }
}
