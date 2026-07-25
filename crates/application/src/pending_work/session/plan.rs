//! Plans a pending-work session before host validation or dispatch.

use std::error::Error;

use thiserror::Error;

use super::{
    Agent, AgentProbe, ClaudeSessionClient, CodexSessionClient, DispatchConfirmation, DispatchMode,
    LaunchDirectives, ModelTierCatalog, RepositorySessionClient, SessionPlan, ZellijSessionClient,
    launch::dispatch_target, model::AgentModel, model_selection::resolve_model,
};
use crate::{
    AppRecordStore, PendingWorkItem,
    pending_work::{
        find::{FindPendingWorkError, find_open_item},
        project_registry::ProjectRegistry,
        session::AgentLaunch,
        update::{self, PreparedPendingWorkUpdate, UpdatePendingWorkError, UpdatePendingWorkItem},
    },
};

/// Requests one provider-neutral session plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanSession {
    pub id: String,
    pub intent: PlanSessionIntent,
    pub mode: DispatchMode,
    pub directives: LaunchDirectives,
    pub agent: Agent,
    pub model_override: AgentModel,
}

/// Selects whether a plan is prepared for dispatch or rendered without effects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanSessionIntent {
    Dispatch { append: Option<String> },
    DryRun,
}

/// Contains the session planning outcome.
#[expect(
    clippy::large_enum_variant,
    reason = "the dispatch outcome keeps the approved prepared-dispatch type unboxed"
)]
pub enum PlanSessionOk {
    Dispatch(PreparedSessionDispatch),
    DryRun(DryRunSession),
}

/// Contains a validated dry-run plan and its exact process argv.
pub struct DryRunSession {
    plan: SessionPlan,
    argv: Vec<String>,
    probe: AgentProbe,
}

impl DryRunSession {
    #[must_use]
    pub fn plan(&self) -> &SessionPlan {
        &self.plan
    }

    #[must_use]
    pub fn argv(&self) -> &[String] {
        &self.argv
    }

    #[must_use]
    pub fn probe(&self) -> &AgentProbe {
        &self.probe
    }
}

/// Contains a session plan and any append prepared for later persistence.
pub struct PreparedSessionDispatch {
    pub(super) plan: SessionPlan,
    confirmation: DispatchConfirmation,
    pub(super) prepared_update: Option<PreparedPendingWorkUpdate>,
    probe: AgentProbe,
}

impl PreparedSessionDispatch {
    /// Returns the provider-neutral session plan.
    #[must_use]
    pub fn plan(&self) -> &SessionPlan {
        &self.plan
    }

    /// Returns the context rendered before an interactive dispatch.
    #[must_use]
    pub fn confirmation(&self) -> &DispatchConfirmation {
        &self.confirmation
    }

    #[must_use]
    pub fn probe(&self) -> &AgentProbe {
        &self.probe
    }
}

/// Reports failures that prevent a session plan from being produced or persisted.
#[derive(Debug, Error)]
pub enum PlanSessionError {
    #[error(transparent)]
    Find(#[from] FindPendingWorkError),
    #[error(transparent)]
    Update(#[from] UpdatePendingWorkError),
    #[error("Pending-work item '{id}' is not launchable: {}", issues.join("; "))]
    NotLaunchable { id: String, issues: Vec<String> },
    #[error("Repo directory for project '{project}' does not exist: {path}")]
    RepositoryMissing { project: String, path: String },
    #[error("zellij not found on PATH; cannot dispatch a pwf session (Linux-only feature).")]
    MultiplexerNotFound,
    /// Preserves the model-catalog adapter's source chain.
    #[error("{0}")]
    ModelTier(#[source] Box<dyn Error + Send + Sync>),
}

/// Plans one open item without performing host I/O or persistence.
///
/// # Errors
///
/// Returns [`PlanSessionError`] for lookup, launch validation, model selection, or append
/// preparation failures.
#[cqrsy::command]
#[expect(
    clippy::too_many_arguments,
    reason = "the operation keeps each concrete external capability visible"
)]
pub fn execute(
    command: &PlanSession,
    store: &impl AppRecordStore<PendingWorkItem>,
    projects: &ProjectRegistry,
    model_tiers: &impl ModelTierCatalog,
    repository: &impl RepositorySessionClient,
    claude: &impl ClaudeSessionClient,
    codex: &impl CodexSessionClient,
    zellij: &impl ZellijSessionClient,
) -> Result<PlanSessionOk, PlanSessionError> {
    let probe = match command.agent {
        Agent::Claude => claude.probe(),
        Agent::Codex => codex.probe(),
    };
    let item = find_open_item(store, projects, &command.id)?;
    if !item.launchable {
        return Err(PlanSessionError::NotLaunchable {
            id: item.id,
            issues: item.issues,
        });
    }

    let model: AgentModel = match command.model_override.clone().into_inner() {
        Some(model) => Some(model),
        None => resolve_model(model_tiers, command.agent, &item.id, item.effort.as_deref())
            .map_err(|error| PlanSessionError::ModelTier(Box::new(error)))?,
    }
    .into();
    let prepared_update = if let PlanSessionIntent::Dispatch {
        append: Some(append),
    } = &command.intent
    {
        Some(update::prepare(
            &UpdatePendingWorkItem {
                id: item.id.clone(),
                prompt: None,
                title: None,
                append: Some(append.clone()),
                prereq: Vec::new(),
                clear_prereq: false,
                commits: Vec::new(),
                append_report: None,
                effort: None,
                tags: Vec::new(),
                tags_clear: false,
            },
            store,
            projects,
        )?)
    } else {
        None
    };
    let target = dispatch_target(&item.id);
    let plan = SessionPlan {
        launch: AgentLaunch::new(
            &item,
            command.directives,
            command.agent,
            model.clone().into_inner(),
        ),
        mode: command.mode,
        target: target.clone(),
    };
    if !repository.is_directory(&plan.launch.repository) {
        return Err(PlanSessionError::RepositoryMissing {
            project: item.project.clone(),
            path: plan.launch.repository.clone(),
        });
    }
    if matches!(command.intent, PlanSessionIntent::Dispatch { .. })
        && command.mode == DispatchMode::Multiplexer
        && !zellij.available()
    {
        return Err(PlanSessionError::MultiplexerNotFound);
    }
    let confirmation = DispatchConfirmation {
        task_id: item.id,
        title: item.session,
        created: item.created,
        mode: command.mode,
        agent: command.agent,
        directives: command.directives,
        model: model.display_or_default(),
        target,
    };

    match command.intent {
        PlanSessionIntent::Dispatch { .. } => {
            Ok(PlanSessionOk::Dispatch(PreparedSessionDispatch {
                plan,
                confirmation,
                prepared_update,
                probe,
            }))
        }
        PlanSessionIntent::DryRun => {
            let provider_argv = match command.agent {
                Agent::Claude => claude.preview(&plan.launch),
                Agent::Codex => codex.preview(&plan.launch),
            };
            let argv = match command.mode {
                DispatchMode::Inline => provider_argv,
                DispatchMode::Multiplexer => zellij.new_tab_process_argv(
                    &plan.target.session,
                    &plan.launch.repository,
                    &plan.target.tab,
                    &provider_argv,
                ),
            };
            Ok(PlanSessionOk::DryRun(DryRunSession { plan, argv, probe }))
        }
    }
}
