use std::{
    fmt,
    num::NonZeroUsize,
    path::{Path, PathBuf},
};

use pwf_models::{
    AppDate,
    project::{ProjectName, ProjectSourceValue},
    task::{
        BlockedBy, CommitRanges, EffortTier, TaskId, TaskPrompt, TaskSection, TaskStatus, TaskTags,
        TaskTitle,
    },
};

pub mod session;

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

/// Describes the result of reopening a task without a redundant boolean flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReopenedTask {
    Reopened { id: TaskId, project: ProjectName },
    AlreadyActive { id: TaskId, project: ProjectName },
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

/// Selects the defaults used by the direct list command or project compatibility route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ListMode {
    #[default]
    Direct,
    ProjectRoute,
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
    pub status: Option<TaskStatus>,
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
        for (index, issue) in self.issues.iter().enumerate() {
            if index > 0 {
                formatter.write_str("; ")?;
            }
            write!(formatter, "{issue}")?;
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
    pub fn try_new(index_path: TaskIndexPath, line: usize) -> Option<Self> {
        if index_path.as_path().as_os_str().is_empty() {
            return None;
        }
        Some(Self {
            index_path,
            line: NonZeroUsize::new(line)?,
        })
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
    pub effort: Option<EffortTier>,
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
