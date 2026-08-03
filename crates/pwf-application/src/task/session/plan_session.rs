//! Plans a task session before host validation or dispatch.

use std::{error::Error, path::PathBuf};

use pwf_models::session::AgentModel;
use thiserror::Error;

use super::{
    Agent, AgentProbe, DispatchConfirmation, DispatchMode, LaunchDirectives, SessionEffort,
    SessionPlan, logic,
};
use crate::{
    ports::{
        agent::AgentClient,
        project_note::ProjectNoteStore,
        repository_directory::RepositoryDirectoryClient,
        session::{AgentCommand, SessionClient, SessionStart, SessionWindow},
        task_record::TaskStore,
    },
    project::{
        get_active_project::{self, GetActiveProject},
        resolve_runtime_path::{self, ResolveRuntimePath},
    },
    task::{
        dto::PreparedTaskUpdate,
        find_active_task::{self, FindActiveTask, FindActiveTaskError},
        identifier,
        logic::task_update,
        show_task::ShowTaskError,
        update_task::{UpdateTask, UpdateTaskError},
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
    pub(super) prepared_update: Option<PreparedTaskUpdate>,
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
    #[error(transparent)]
    Update(#[from] UpdateTaskError),
    #[error(transparent)]
    Show(#[from] ShowTaskError),
    #[error("Task '{id}' is not launchable: {}", issues.join("; "))]
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
    #[error("Invalid repository path for project '{project}': {reason}")]
    InvalidRepositoryPath { project: String, reason: String },
}

/// Plans one active task without performing host I/O or persistence.
///
/// # Errors
///
/// Returns [`PlanSessionError`] for lookup, launch validation, model selection, or append
/// preparation failures.
#[cqrsy::command]
#[expect(
    clippy::too_many_lines,
    reason = "session planning validates one cohesive launch"
)]
pub async fn execute(
    command: &PlanSession,
    store: &(impl TaskStore + ProjectNoteStore),
    pool: &sqlx::SqlitePool,
    home: &PathBuf,
    agent_client: &impl AgentClient,
    repository: &impl RepositoryDirectoryClient,
    session_client: &impl SessionClient,
) -> Result<PlanSessionOk, PlanSessionError> {
    let probe = agent_client.probe(command.agent);
    let mut task = find_active_task::execute(
        &FindActiveTask {
            id: command.id.clone(),
        },
        store,
        pool,
    )
    .await?;
    if !task.launchable {
        return Err(PlanSessionError::NotLaunchable {
            id: task.id,
            issues: task.issues,
        });
    }
    let source_path = task
        .repo
        .clone()
        .ok_or_else(|| PlanSessionError::InvalidRepositoryPath {
            project: task.project.clone(),
            reason: "project has no directory source".to_string(),
        })?;
    let resolved_repository = resolve_runtime_path::execute(&ResolveRuntimePath {
        path: source_path,
        home: home.clone(),
    })
    .map_err(|error| PlanSessionError::InvalidRepositoryPath {
        project: task.project.clone(),
        reason: error.to_string(),
    })?;
    task.repo = Some(resolved_repository.path().to_string_lossy().into_owned());

    let model: AgentModel = match command.model_override.clone().into_inner() {
        Some(model) => Some(model),
        None => logic::resolve_model(command.agent, &task.id, task.effort.as_deref(), |effort| {
            agent_client.model_tier(effort)
        })
        .map_err(|error| PlanSessionError::ModelTier(Box::new(error)))?,
    }
    .into();
    let prepared_update = prepare_append(command, &task.id, store, pool).await?;
    let target = logic::dispatch_target(&task.id);
    let task_content = logic::load_task_content(&task.id, store, pool).await?;
    let plan = SessionPlan {
        launch: logic::agent_launch(
            &task,
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
            project: task.project.clone(),
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
        task_id: task.id,
        title: task.session,
        created: task.created,
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

async fn prepare_append(
    command: &PlanSession,
    task_id: &str,
    store: &impl TaskStore,
    pool: &sqlx::SqlitePool,
) -> Result<Option<PreparedTaskUpdate>, UpdateTaskError> {
    let PlanSessionIntent::Dispatch {
        append: Some(append),
    } = &command.intent
    else {
        return Ok(None);
    };
    let id = identifier::parse(task_id).expect("planned session task has a canonical id");
    let project = get_active_project::execute(
        GetActiveProject {
            id: id.project_id(),
        },
        pool,
    )
    .await
    .map_err(|error| UpdateTaskError::QueryProject(Box::new(error)))?;
    task_update::prepare(
        &UpdateTask {
            id: task_id.to_string(),
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
        &project,
        std::slice::from_ref(&project),
    )
    .map(Some)
}
