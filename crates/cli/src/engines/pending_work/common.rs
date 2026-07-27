use clap::Args;
use pwf_application::pending_work::{StatusFilter, session::Agent};
use pwf_domain::pending_work::{EffortTier, WorkItemStatus};
use thiserror::Error;

#[derive(Args, Clone, Debug, Default)]
pub struct CommonArguments {
    /// Date stamp (YYYY-MM-DD); defaults to today.
    #[arg(long)]
    pub(crate) date: Option<String>,
}

#[derive(Args, Debug, Default)]
pub struct Identifier {
    /// Item id (bare positional; `--id` also accepted). E.g. `PWF-0001`, `cfg57`.
    #[arg(value_name = "ID")]
    positional: Option<String>,
    #[arg(long = "id", value_name = "ID", conflicts_with = "positional")]
    flag: Option<String>,
}

impl Identifier {
    pub(crate) fn from_positional(value: Option<String>) -> Self {
        Self {
            positional: value,
            flag: None,
        }
    }

    pub(crate) fn raw(&self) -> Option<&str> {
        self.positional.as_deref().or(self.flag.as_deref())
    }

    pub(crate) fn required(&self, action: &'static str) -> Result<String, PendingWorkError> {
        self.raw()
            .map(str::to_string)
            .ok_or(PendingWorkError::MissingId { action })
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
            Self::Active => StatusFilter::Exact(WorkItemStatus::Active),
            Self::Done => StatusFilter::Exact(WorkItemStatus::Done),
            Self::Cancelled => StatusFilter::Exact(WorkItemStatus::Cancelled),
            Self::All => StatusFilter::All,
        }
    }
}

/// Carries pending-work errors across CLI and in-process handoff seams.
/// The handoff seam converts these errors with `to_string()` instead of matching variants.
#[derive(Debug, Error)]
pub(crate) enum PendingWorkError {
    #[error("{0}")]
    ApplicationList(String),
    #[error("{0}")]
    ApplicationRead(String),
    #[error("{0}")]
    ApplicationWrite(String),
    #[error(transparent)]
    Add(#[from] pwf_application::pending_work::add_pending_work_item::AddPendingWorkError),
    #[error(transparent)]
    Complete(
        #[from] pwf_application::pending_work::complete_pending_work::CompletePendingWorkError,
    ),
    #[error(transparent)]
    Cancel(#[from] pwf_application::pending_work::cancel_pending_work::CancelPendingWorkError),
    #[error(transparent)]
    Reopen(#[from] pwf_application::pending_work::reopen_pending_work::ReopenPendingWorkError),
    #[error(transparent)]
    Remove(#[from] pwf_application::pending_work::remove_pending_work_item::RemovePendingWorkError),
    #[error(transparent)]
    SessionPlan(#[from] pwf_application::pending_work::session::plan_session::PlanSessionError),
    #[error(transparent)]
    SessionDispatch(
        #[from] pwf_application::pending_work::session::dispatch_session::DispatchSessionError,
    ),
    #[error(transparent)]
    SessionVerify(
        #[from] pwf_application::pending_work::session::verify_session::VerifySessionError,
    ),
    #[error("Unknown --section value '{value}'. Use one of: future, human, low-prio.")]
    BadSection { value: String },
    #[error(
        "'{identifier}' is ambiguous. Managed project identifiers matching it: {}.",
        matches.join(", ")
    )]
    AmbiguousManagedProject {
        identifier: String,
        matches: Vec<String>,
    },
    #[error(
        "Unknown managed project identifier: {identifier}\nManaged project identifiers: {}",
        known.join(", ")
    )]
    UnknownManagedProject {
        identifier: String,
        known: Vec<String>,
    },
    #[error("Open pending-work item not found: {id}")]
    ItemNotFound { id: String },
    #[error("--id is required for {action}.")]
    MissingId { action: &'static str },
    #[error("--report cannot be empty.")]
    EmptyReport,
    #[error("--report is required for cancel.")]
    MissingCancelReport,
    #[error("remove only supports file-model pending-work items.")]
    RemoveRequiresFileModel,
    #[error("Work-item note missing: {}", path.display())]
    WorkItemNoteMissing { path: std::path::PathBuf },
    #[error(
        "Invalid --tag value {raw:?}; use lowercase/uppercase ASCII letters, digits, '_' or '-', without leading, trailing, or repeated separators."
    )]
    InvalidTag { raw: String },
    #[error("{}", ADD_HINT)]
    RouteCreateRejected,
    /// Reports an application-owned handoff preflight failure.
    #[error(transparent)]
    HandoffLifecycle(#[from] pwf_application::handoff::HandoffError),
    /// Reports a pending-work mutation followed by an application-owned handoff failure.
    #[error("{id} was mutated, but its handoff was not: {source}\n  {remedy}")]
    HandoffLifecycleAfterMutation {
        id: String,
        source: pwf_application::handoff::HandoffError,
        remedy: String,
    },
}

impl From<PendingWorkError> for String {
    fn from(error: PendingWorkError) -> Self {
        error.to_string()
    }
}

pub(super) const ADD_HINT: &str = r#"Use: pwf add <project> "<prompt>""#;
