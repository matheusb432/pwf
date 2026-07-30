//! Plans a pending-work session before host validation or dispatch.

use std::error::Error;

use pwf_models::session::AgentModel;
use thiserror::Error;

use super::{
    Agent, AgentProbe, DispatchConfirmation, DispatchMode, LaunchDirectives, SessionEffort,
    SessionPlan, logic,
};
use crate::{
    AgentClient, AgentCommand, AppRecordStore, PendingWorkRecord, ProjectNoteStore,
    RepositoryDirectoryClient, SessionClient, SessionStart, SessionWindow,
    pending_work::{
        ProjectRegistry,
        dto::PreparedPendingWorkUpdate,
        find_pending_work::FindPendingWorkError,
        logic::{finding::find_open_item, pending_work_update},
        show_pending_work_item::ShowPendingWorkError,
        update_pending_work_item::{UpdatePendingWorkError, UpdatePendingWorkItem},
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
    pub effort: SessionEffort,
}

/// Selects whether a plan is prepared for dispatch or rendered without effects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanSessionIntent {
    Dispatch { append: Option<String> },
    DryRun,
}

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
    pub(super) confirmation: DispatchConfirmation,
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

#[derive(Debug, Error)]
pub enum PlanSessionError {
    #[error(transparent)]
    Find(#[from] FindPendingWorkError),
    #[error(transparent)]
    Update(#[from] UpdatePendingWorkError),
    #[error(transparent)]
    Show(#[from] ShowPendingWorkError),
    #[error("Pending-work item '{id}' is not launchable: {}", issues.join("; "))]
    NotLaunchable { id: String, issues: Vec<String> },
    #[error("Repo directory for project '{project}' does not exist: {path}")]
    RepositoryMissing { project: String, path: String },
    #[error("Session multiplexer is unavailable; cannot dispatch a pwf session.")]
    MultiplexerNotFound,
    #[error("Checking multiplexer session '{session}' failed: {message}")]
    MultiplexerSessionCheck { session: String, message: String },
    #[error("Multiplexer session '{session}' does not exist")]
    MultiplexerSessionMissing {
        session: String,
        start_command_argv: Vec<String>,
    },
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
pub fn execute(
    command: &PlanSession,
    store: &(impl AppRecordStore<PendingWorkRecord> + ProjectNoteStore),
    projects: &ProjectRegistry,
    agent_client: &impl AgentClient,
    repository: &impl RepositoryDirectoryClient,
    session_client: &impl SessionClient,
) -> Result<PlanSessionOk, PlanSessionError> {
    let probe = agent_client.probe(command.agent);
    let item = find_open_item(store, projects, &command.id)?;
    if !item.launchable {
        return Err(PlanSessionError::NotLaunchable {
            id: item.id,
            issues: item.issues,
        });
    }

    let model: AgentModel = match command.model_override.clone().into_inner() {
        Some(model) => Some(model),
        None => logic::resolve_model(command.agent, &item.id, item.effort.as_deref(), |effort| {
            agent_client.model_tier(effort)
        })
        .map_err(|error| PlanSessionError::ModelTier(Box::new(error)))?,
    }
    .into();
    let prepared_update = prepare_append(command, &item.id, store, projects)?;
    let target = logic::dispatch_target(&item.id);
    let task_content = logic::load_task_content(&item.id, store, projects)?;
    let plan = SessionPlan {
        launch: logic::agent_launch(
            &item,
            &task_content,
            command.directives,
            command.agent,
            model.clone().into_inner(),
            command.effort,
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
    {
        if !session_client.available() {
            return Err(PlanSessionError::MultiplexerNotFound);
        }
        let session_exists = session_client
            .session_exists(&plan.target.session)
            .map_err(|message| PlanSessionError::MultiplexerSessionCheck {
                session: plan.target.session.clone(),
                message,
            })?;
        if !session_exists {
            let start = SessionStart::builder()
                .session_name(&plan.target.session)
                .working_directory(&plan.launch.repository)
                .build();
            return Err(PlanSessionError::MultiplexerSessionMissing {
                session: plan.target.session.clone(),
                start_command_argv: session_client.preview_start(&start),
            });
        }
    }
    let confirmation = DispatchConfirmation {
        task_id: item.id,
        title: item.session,
        created: item.created,
        mode: command.mode,
        agent: command.agent,
        directives: command.directives,
        model: model.display_or_default(),
        effort: command.effort,
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
            let provider_argv = agent_client.preview(&plan.launch);
            let argv = match command.mode {
                DispatchMode::Inline => provider_argv,
                DispatchMode::Multiplexer => {
                    let window = SessionWindow::builder()
                        .session_name(&plan.target.session)
                        .working_directory(&plan.launch.repository)
                        .window_name(&plan.target.window)
                        .agent_command(AgentCommand::new(&provider_argv))
                        .build();
                    session_client.preview_window(&window)
                }
            };
            Ok(PlanSessionOk::DryRun(DryRunSession { plan, argv, probe }))
        }
    }
}

fn prepare_append(
    command: &PlanSession,
    item_id: &str,
    store: &impl AppRecordStore<PendingWorkRecord>,
    projects: &ProjectRegistry,
) -> Result<Option<PreparedPendingWorkUpdate>, UpdatePendingWorkError> {
    let PlanSessionIntent::Dispatch {
        append: Some(append),
    } = &command.intent
    else {
        return Ok(None);
    };
    pending_work_update::prepare(
        &UpdatePendingWorkItem {
            id: item_id.to_string(),
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
    )
    .map(Some)
}
