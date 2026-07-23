use clap::Args;
use pwf_application::pending_work::session::Agent;
use pwf_domain::pending_work::{WorkItemStatus, WorkItemStatusFilter, canonical_pending_id};
use thiserror::Error;

use crate::config::Config;

#[derive(Args, Clone, Debug, Default)]
pub struct CommonArguments {
    /// Path to the pwf config JSON (overrides $`PWF_CONFIG`).
    #[arg(long)]
    pub(crate) config_path: Option<String>,
    /// Override the notes directory.
    #[arg(long)]
    pub(crate) notes_dir: Option<String>,
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

    pub(crate) fn canonical(&self) -> Option<String> {
        self.raw().map(canonical_pending_id)
    }

    pub(crate) fn required(&self, action: &'static str) -> Result<String, PendingWorkError> {
        self.canonical()
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

impl StatusChoice {
    pub(crate) fn filter(self) -> WorkItemStatusFilter {
        match self {
            Self::Active => WorkItemStatusFilter::Exact(WorkItemStatus::Active),
            Self::Done => WorkItemStatusFilter::Exact(WorkItemStatus::Done),
            Self::Cancelled => WorkItemStatusFilter::Exact(WorkItemStatus::Cancelled),
            Self::All => WorkItemStatusFilter::All,
        }
    }
}

pub(crate) fn load_configuration(arguments: &CommonArguments) -> Result<Config, PendingWorkError> {
    let config_path = arguments
        .config_path
        .clone()
        .or_else(crate::config::default_config_path)
        .ok_or(PendingWorkError::MissingConfigPath)?;
    Ok(crate::config::load(
        &config_path,
        arguments.notes_dir.as_deref(),
    )?)
}

/// Carries pending-work errors across CLI and in-process handoff seams.
/// The handoff seam converts these errors with `to_string()` instead of matching variants.
#[derive(Debug, Error)]
pub(crate) enum PendingWorkError {
    #[error("{0}")]
    Config(
        #[from]
        #[source]
        crate::config::ConfigError,
    ),
    #[error("missing --config-path")]
    MissingConfigPath,
    #[error("{0}")]
    ApplicationList(String),
    #[error("{0}")]
    ApplicationRead(String),
    #[error("{0}")]
    ApplicationWrite(String),
    #[error(transparent)]
    Add(#[from] pwf_application::pending_work::add::AddPendingWorkError),
    #[error(transparent)]
    Complete(#[from] pwf_application::pending_work::done::CompletePendingWorkError),
    #[error(transparent)]
    Cancel(#[from] pwf_application::pending_work::cancel::CancelPendingWorkError),
    #[error(transparent)]
    Reopen(#[from] pwf_application::pending_work::reopen::ReopenPendingWorkError),
    #[error(transparent)]
    Remove(#[from] pwf_application::pending_work::remove::RemovePendingWorkError),
    #[error(transparent)]
    SessionPlan(#[from] pwf_application::pending_work::session::plan::PlanSessionError),
    #[error(transparent)]
    SessionDispatch(#[from] pwf_application::pending_work::session::dispatch::DispatchSessionError),
    #[error(transparent)]
    SessionVerify(#[from] pwf_application::pending_work::session::verify::VerifySessionError),
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
