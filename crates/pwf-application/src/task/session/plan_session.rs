//! Plans a task session before host validation or dispatch.

use std::{
    error::Error,
    path::{Path, PathBuf},
};

use pwf_models::{
    project::{ProjectId, ProjectSourceValue},
    session::{AgentModel, PushedPrompt},
    task::TaskId,
};
use thiserror::Error;

use super::{
    Agent, AgentLaunch, AgentProbe, DispatchConfirmation, DispatchMode, LaunchDirectives,
    SessionEffort, SessionPlan, logic,
};
use crate::{
    ports::{
        agent::AgentClient,
        project_directory::ProjectDirectoryClient,
        project_note::ProjectNoteStore,
        session::{AgentCommand, SessionClient, SessionStart, SessionWindow},
        task_record::{Materialization, TaskRecord, TaskStore},
    },
    project::resolve_runtime_path::{self, ResolveRuntimePath},
    task::find_active_task::{self, FindActiveTask, FindActiveTaskError},
};

/// Requests one provider-neutral session plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanSession {
    pub task_id: TaskId,
    pub intent: PlanSessionIntent,
    pub pushed_prompt: Option<PushedPrompt>,
    pub mode: DispatchMode,
    pub directives: LaunchDirectives,
    pub agent: Agent,
    pub model_override: AgentModel,
    pub effort: SessionEffort,
}

/// Selects whether a plan is prepared for dispatch or rendered without effects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanSessionIntent {
    Dispatch,
    DryRun,
}

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

/// Contains a validated session dispatch ready for confirmation.
pub struct PreparedSessionDispatch {
    pub(super) plan: SessionPlan,
    pub(super) confirmation: DispatchConfirmation,
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
    Find(#[from] FindActiveTaskError),
    #[error("{0}")]
    ReadTaskMarkdown(#[source] Box<dyn Error + Send + Sync>),
    #[error("Task '{id}' is not launchable: {}", issues.join("; "))]
    NotLaunchable { id: TaskId, issues: Vec<String> },
    #[error("Project path for '{project_id}' does not exist: {path}")]
    ProjectPathMissing { project_id: ProjectId, path: String },
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
    #[error("Failed to render session title: {0}")]
    RenderThreadTitle(#[source] askama::Error),
    #[error("Invalid path for project '{project_id}': {reason}")]
    InvalidProjectPath {
        project_id: ProjectId,
        reason: String,
    },
}

/// Plans one active task without performing host I/O or persistence.
///
/// # Errors
///
/// Returns [`PlanSessionError`] for lookup, launch validation, or model selection failures.
#[cqrsy::command]
pub async fn execute(
    command: &PlanSession,
    store: &(impl TaskStore + ProjectNoteStore),
    pool: &sqlx::SqlitePool,
    home: &PathBuf,
    agent_client: &impl AgentClient,
    project_directory: &impl ProjectDirectoryClient,
    session_client: &impl SessionClient,
) -> Result<PlanSessionOk, PlanSessionError> {
    let probe = agent_client.probe(command.agent);
    let found = find_active_task::execute(
        &FindActiveTask {
            id: command.task_id.clone(),
        },
        store,
        pool,
    )
    .await?;
    let task = found.task;
    if !task.launchable {
        return Err(PlanSessionError::NotLaunchable {
            id: command.task_id.clone(),
            issues: task.issues,
        });
    }
    let project_id = found.project.id.clone();
    let project_path = resolve_project_path(found.project.source.value(), &project_id, home)?;
    let task_content = load_task_content(&found.record, store)?;

    let model: AgentModel = match command.model_override.clone().into_inner() {
        Some(model) => Some(model),
        None => logic::resolve_model(
            command.agent,
            &command.task_id,
            task.effort.as_deref(),
            |effort| agent_client.model_tier(effort),
        )
        .map_err(|error| PlanSessionError::ModelTier(Box::new(error)))?,
    }
    .into();
    let target = logic::dispatch_target(&command.task_id);
    let plan = SessionPlan {
        launch: AgentLaunch {
            agent: command.agent,
            task_id: command.task_id.clone(),
            title: logic::thread_title(
                &task,
                &command.task_id,
                command.directives,
                command.agent,
                command.effort,
            )
            .map_err(PlanSessionError::RenderThreadTitle)?,
            project_path: project_path.clone(),
            prompt: logic::launch_prompt(
                &task_content,
                &command.task_id,
                command.pushed_prompt.as_ref(),
                command.directives,
            ),
            model: model.clone().into_inner(),
            effort: command.effort,
        },
        mode: command.mode,
        target: target.clone(),
    };
    validate_project_path(project_directory, &project_id, &plan.launch.project_path)?;
    validate_multiplexer(command, &plan, session_client)?;
    let confirmation = DispatchConfirmation {
        task_id: command.task_id.clone(),
        title: task.session,
        created: task.created,
        mode: command.mode,
        agent: command.agent,
        directives: command.directives,
        has_pushed_prompt: command.pushed_prompt.is_some(),
        model: model.display_or_default(),
        effort: command.effort,
        target,
    };

    match command.intent {
        PlanSessionIntent::Dispatch => Ok(PlanSessionOk::Dispatch(PreparedSessionDispatch {
            plan,
            confirmation,
            probe,
        })),
        PlanSessionIntent::DryRun => {
            let provider_argv = agent_client.preview(&plan.launch);
            let argv = match command.mode {
                DispatchMode::Inline => provider_argv,
                DispatchMode::Multiplexer => {
                    let session_name = plan.target.session_name();
                    let window = SessionWindow::new(
                        &session_name,
                        &plan.launch.project_path,
                        plan.target.task_id.as_ref(),
                        AgentCommand::new(&provider_argv),
                    );
                    session_client.preview_window(&window)
                }
            };
            Ok(PlanSessionOk::DryRun(DryRunSession { plan, argv, probe }))
        }
    }
}

fn load_task_content(
    record: &TaskRecord,
    store: &impl ProjectNoteStore,
) -> Result<String, PlanSessionError> {
    if matches!(record.materialization, Materialization::MissingNote { .. }) {
        return store
            .read_note_markdown(&record.locator)
            .map_err(|error| PlanSessionError::ReadTaskMarkdown(Box::new(error)));
    }
    Ok(record.source.clone())
}

fn resolve_project_path(
    source_value: &ProjectSourceValue,
    project_id: &ProjectId,
    home: &Path,
) -> Result<String, PlanSessionError> {
    let resolved = resolve_runtime_path::execute(&ResolveRuntimePath {
        path: source_value.as_ref().to_string(),
        home: home.to_path_buf(),
    })
    .map_err(|error| PlanSessionError::InvalidProjectPath {
        project_id: project_id.clone(),
        reason: error.to_string(),
    })?;
    Ok(resolved.path().to_string_lossy().into_owned())
}

fn validate_project_path(
    project_directory: &impl ProjectDirectoryClient,
    project_id: &ProjectId,
    project_path: &str,
) -> Result<(), PlanSessionError> {
    if project_directory.is_directory(project_path) {
        return Ok(());
    }
    Err(PlanSessionError::ProjectPathMissing {
        project_id: project_id.clone(),
        path: project_path.to_string(),
    })
}

fn validate_multiplexer(
    command: &PlanSession,
    plan: &SessionPlan,
    session_client: &impl SessionClient,
) -> Result<(), PlanSessionError> {
    if !matches!(command.intent, PlanSessionIntent::Dispatch)
        || command.mode != DispatchMode::Multiplexer
    {
        return Ok(());
    }
    if !session_client.available() {
        return Err(PlanSessionError::MultiplexerNotFound);
    }
    let session_name = plan.target.session_name();
    let session_exists = session_client
        .session_exists(&session_name)
        .map_err(|message| PlanSessionError::MultiplexerSessionCheck {
            session: session_name.clone(),
            message,
        })?;
    if session_exists {
        return Ok(());
    }
    let start = SessionStart::new(&session_name, &plan.launch.project_path);
    let start_command_argv = session_client.preview_start(&start);
    Err(PlanSessionError::MultiplexerSessionMissing {
        session: session_name,
        start_command_argv,
    })
}
