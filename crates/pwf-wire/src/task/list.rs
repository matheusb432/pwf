use std::{fmt, num::NonZeroUsize};

use pwf_models::{
    AppDate,
    project::{ProjectId, ProjectName, ProjectSourceValue},
    task::{
        BlockedBy, EffortTier, PriorityTier, TaskId, TaskPrompt, TaskSection, TaskStatus, TaskTags,
        TaskTitle, order::OrderSpec,
    },
};

use super::{ProjectTaskPath, RawTaskTags, TaskIndexPath, TaskNotePath};

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
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ListScope {
    #[default]
    Default,
    Section(TaskSection),
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
pub struct ListedTask {
    pub id: TaskId,
    pub project: ProjectName,
    pub status: TaskStatus,
    pub heading: TaskHeading,
    pub section: Option<TaskSection>,
    pub effort: Option<EffortTier>,
    pub priority: Option<PriorityTier>,
    pub tags: Option<RawTaskTags>,
    pub created: Option<AppDate>,
    pub details: Option<ListedTaskDetails>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedTaskDetails {
    pub prompt: TaskPrompt,
    pub project_path: Option<ProjectSourceValue>,
    pub location: TaskLocation,
    pub launch: TaskLaunch,
    pub blocked_by: Option<BlockedBy>,
    pub blocked_by_statuses: Vec<BlockedByStatus>,
    pub blocked_by_issues: Vec<BlockedByIssue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedTasks {
    pub tasks: Vec<ListedTask>,
    pub hidden: usize,
    pub project: Option<ProjectName>,
    pub project_task_path: Option<ProjectTaskPath>,
    pub status_filter: StatusFilter,
    pub layout: ListLayout,
    pub detail: ListDetail,
    pub next_page_token: Option<TaskPageToken>,
}

#[derive(Debug, Clone)]
pub struct ListTasks {
    pub project_id: Option<ProjectId>,
    pub scope: ListScope,
    /// Explicit task cap. Omission uses the mode-specific default.
    pub number: Option<TaskListLimit>,
    pub effort: Option<EffortTier>,
    pub priority: Option<PriorityTier>,
    pub tags: Option<TaskTags>,
    pub order: Option<OrderSpec>,
    /// Explicit lifecycle filter. Omission uses the mode-specific default.
    pub status: Option<StatusFilter>,
    pub detail: ListDetail,
    pub page_size: Option<TaskPageSize>,
    pub page_token: Option<TaskPageToken>,
}

/// Caps one task-list response before transport message limits apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskListLimit(NonZeroUsize);

impl TaskListLimit {
    pub const MAX: usize = 100_000;

    /// Constructs a nonzero task-list limit within the supported response cap.
    ///
    /// # Errors
    ///
    /// Returns [`TaskListLimitError`] when `value` is zero or exceeds [`Self::MAX`].
    pub fn try_new(value: usize) -> Result<Self, TaskListLimitError> {
        NonZeroUsize::new(value)
            .filter(|value| value.get() <= Self::MAX)
            .map(Self)
            .ok_or(TaskListLimitError { value })
    }

    #[must_use]
    pub const fn get(self) -> usize {
        self.0.get()
    }
}

/// Reports a task-list limit outside the supported response cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{value} must be between 1 and {}", TaskListLimit::MAX)]
pub struct TaskListLimitError {
    value: usize,
}

/// Caps one page of task-list results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskPageSize(NonZeroUsize);

impl TaskPageSize {
    pub const DEFAULT: usize = 100;
    pub const MAX: usize = 256;

    pub fn try_new(value: usize) -> Result<Self, TaskPageSizeError> {
        NonZeroUsize::new(value)
            .filter(|value| value.get() <= Self::MAX)
            .map(Self)
            .ok_or(TaskPageSizeError { value })
    }

    #[must_use]
    pub const fn get(self) -> usize {
        self.0.get()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{value} must be between 1 and {}", TaskPageSize::MAX)]
pub struct TaskPageSizeError {
    value: usize,
}

/// Carries one opaque bounded task-list continuation token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskPageToken(Box<str>);

impl TaskPageToken {
    pub const MAX_LEN: usize = 2_048;

    pub fn try_new(value: impl Into<String>) -> Result<Self, TaskPageTokenError> {
        let value = value.into();
        if value.is_empty() || value.len() > Self::MAX_LEN {
            return Err(TaskPageTokenError);
        }
        Ok(Self(value.into_boxed_str()))
    }
}

impl AsRef<str> for TaskPageToken {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for TaskPageToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("must contain between 1 and {} characters", TaskPageToken::MAX_LEN)]
pub struct TaskPageTokenError;
