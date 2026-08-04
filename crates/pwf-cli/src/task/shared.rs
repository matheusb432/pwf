use clap::Args;
use pwf_application::task::StatusFilter;
use pwf_models::{
    project::ProjectSelector,
    session::Agent,
    task::{EffortTier, TaskId, TaskStatus, TaskTitle, TaskTitleError},
};
use thiserror::Error;

#[derive(Args, Clone, Debug, Default)]
pub struct CommonArguments {
    /// Date stamp (YYYY-MM-DD); defaults to today.
    #[arg(long)]
    pub(crate) date: Option<String>,
}

#[derive(Args, Debug, Default)]
pub struct Identifier {
    /// Task id (bare positional; `--id` also accepted). E.g. `PWF-0001`, `cfg57`.
    #[arg(value_name = "ID")]
    positional: Option<TaskId>,
    #[arg(long = "id", value_name = "ID", conflicts_with = "positional")]
    flag: Option<TaskId>,
}

impl Identifier {
    pub(crate) fn required(&self, action: &'static str) -> Result<TaskId, TaskError> {
        self.positional
            .as_ref()
            .or(self.flag.as_ref())
            .cloned()
            .ok_or(TaskError::MissingId { action })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum AgentChoice {
    Claude,
    // TODO: make default agent choice be configurable by user
    #[default]
    Codex,
}

impl From<AgentChoice> for Agent {
    fn from(choice: AgentChoice) -> Self {
        match choice {
            AgentChoice::Claude => Self::Claude,
            AgentChoice::Codex => Self::Codex,
        }
    }
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum SectionChoice {
    Future,
    Human,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum StatusChoice {
    Active,
    Done,
    Cancelled,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum EffortChoice {
    Low,
    Medium,
    High,
    Highest,
}

impl From<EffortChoice> for EffortTier {
    fn from(choice: EffortChoice) -> Self {
        match choice {
            EffortChoice::Low => Self::Low,
            EffortChoice::Medium => Self::Medium,
            EffortChoice::High => Self::High,
            EffortChoice::Highest => Self::Highest,
        }
    }
}

impl StatusChoice {
    pub(crate) fn filter(self) -> StatusFilter {
        match self {
            Self::Active => StatusFilter::Exact(TaskStatus::Active),
            Self::Done => StatusFilter::Exact(TaskStatus::Done),
            Self::Cancelled => StatusFilter::Exact(TaskStatus::Cancelled),
            Self::All => StatusFilter::All,
        }
    }
}

#[derive(Debug, Error)]
pub(crate) enum TaskError {
    #[error("{0}")]
    ApplicationList(String),
    #[error("{0}")]
    ApplicationRead(String),
    #[error("{0}")]
    ApplicationWrite(String),
    #[error(transparent)]
    Add(#[from] pwf_application::task::add_task::AddTaskError),
    #[error(transparent)]
    InvalidTitle(#[from] TaskTitleError),
    #[error(transparent)]
    Complete(#[from] pwf_application::task::complete_task::CompleteTaskError),
    #[error(transparent)]
    Cancel(#[from] pwf_application::task::cancel_task::CancelTaskError),
    #[error(transparent)]
    Reopen(#[from] pwf_application::task::reopen_task::ReopenTaskError),
    #[error(transparent)]
    Remove(#[from] pwf_application::task::remove_task::RemoveTaskError),
    #[error(transparent)]
    SessionPlan(pwf_application::task::session::plan_session::PlanSessionError),
    #[error("tmux session '{session}' does not exist.\nStart it with:\n{start_command}")]
    TmuxSessionMissing {
        session: String,
        start_command: String,
    },
    #[error(transparent)]
    SessionDispatch(#[from] pwf_application::task::session::dispatch_session::DispatchSessionError),
    #[error(
        "Unknown managed project identifier: {selector}\nManaged project identifiers: {}",
        known.join(", ")
    )]
    UnknownManagedProject {
        selector: ProjectSelector,
        known: Vec<String>,
    },
    #[error("Task not found: {id}")]
    TaskNotFound { id: TaskId },
    #[error("--id is required for {action}.")]
    MissingId { action: &'static str },
    #[error("--report cannot be empty.")]
    EmptyReport,
    #[error("--report is required for cancel.")]
    MissingCancelReport,
    #[error("Task note missing: {}", path.display())]
    TaskNoteMissing { path: std::path::PathBuf },
    #[error(
        "Invalid --tag value {raw:?}; use lowercase/uppercase ASCII letters, digits, '_' or '-', without leading, trailing, or repeated separators."
    )]
    InvalidTag { raw: String },
    #[error("{}", ADD_HINT)]
    RejectUnsupportedTaskCreation,
}

impl From<TaskError> for String {
    fn from(error: TaskError) -> Self {
        error.to_string()
    }
}

pub(super) const ADD_HINT: &str = r#"Use: pwf task add <project> "<prompt>""#;

pub(super) fn task_title(raw: &str) -> Result<(TaskTitle, bool), TaskError> {
    let comparison = raw.trim().to_lowercase();
    let title = TaskTitle::try_new(raw)?;
    let normalized = title.as_ref() != comparison;
    Ok((title, normalized))
}

#[cfg(test)]
mod tests {
    use super::task_title;

    #[test]
    fn task_title_reports_metadata_normalization_only() {
        for (raw, expected, normalized) in [
            ("  Fix The THING  ", "fix the thing", false),
            ("time is 3:30pm", "time is 3:30pm", false),
            (
                "fix parser: handle colons",
                "fix parser; handle colons",
                true,
            ),
            ("", "n/a", true),
        ] {
            let (title, was_normalized) = task_title(raw).unwrap();
            assert_eq!(title.as_ref(), expected);
            assert_eq!(was_normalized, normalized);
        }
    }
}
