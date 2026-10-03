use std::{fmt, num::NonZeroUsize};

use pwf_models::{
    AppDate,
    project::{ProjectId, ProjectName, ProjectSourceValue},
    task::{
        BlockedBy, EffortTier, PriorityTier, TaskBody, TaskId, TaskListLimit, TaskStatus, TaskTags,
        TaskTimestamp, TaskTitle, order::OrderSpec,
    },
};

use super::{ProjectTaskPath, RawTaskTags, TaskFilePath};

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

/// Selects summary, complete content, or body-and-metadata task-list output.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ListDetail {
    #[default]
    Summary,
    Detailed,
    Preview,
}

impl ListDetail {
    #[must_use]
    pub const fn includes_relationship_statuses(self) -> bool {
        matches!(self, Self::Detailed)
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
        path: TaskFilePath,
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
    PlaceholderBody,
}

impl fmt::Display for TaskIssue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PlaceholderBody => {
                formatter.write_str("Body is a placeholder; define a real body before launching.")
            }
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
    pub fn needs_body(&self) -> bool {
        self.issues.contains(&TaskIssue::PlaceholderBody)
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedTask {
    pub id: TaskId,
    pub project: ProjectName,
    pub status: TaskStatus,
    pub heading: TaskHeading,
    pub effort: Option<EffortTier>,
    pub priority: Option<PriorityTier>,
    pub tags: Option<RawTaskTags>,
    pub created: Option<AppDate>,
    pub details: Option<ListedTaskDetails>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedTaskDetails {
    /// Exact file contents, omitted from preview listings.
    pub source: Option<String>,
    pub created_at: Option<TaskTimestamp>,
    pub completed_at: Option<TaskTimestamp>,
    pub commits: Option<String>,
    pub body: TaskBody,
    pub project_path: Option<ProjectSourceValue>,
    pub file_path: TaskFilePath,
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
    pub detail: ListDetail,
    pub next_page_token: Option<TaskPageToken>,
}

#[derive(Debug, Clone)]
pub struct ListTasks {
    pub project_id: Option<ProjectId>,
    pub all: bool,
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

#[cfg(test)]
mod tests {
    use super::{BlockedByResolution, StatusFilter, TaskStatus};

    #[test]
    fn backlog_is_hidden_by_default_but_remains_an_unresolved_blocker() {
        assert!(!StatusFilter::default().includes(TaskStatus::Backlog));
        assert!(StatusFilter::Exact(TaskStatus::Backlog).includes(TaskStatus::Backlog));
        assert!(StatusFilter::All.includes(TaskStatus::Backlog));
        assert!(BlockedByResolution::Found(TaskStatus::Backlog).is_warning());
    }
}
