//! Plans a task session before host validation or dispatch.

use std::{error::Error, path::PathBuf};

use pwf_models::{
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
        project_note::ProjectNoteStore,
        repository_directory::RepositoryDirectoryClient,
        session::{AgentCommand, SessionClient, SessionStart, SessionWindow},
        task_record::TaskStore,
    },
    project::resolve_runtime_path::{self, ResolveRuntimePath},
    task::{
        find_active_task::{self, FindActiveTask, FindActiveTaskError},
        show_task::ShowTaskError,
    },
};

/// Requests one provider-neutral session plan.
#[derive(Debug, Clone, PartialEq, Eq, bon::Builder)]
pub struct PlanSession {
    #[builder(start_fn, into)]
    task_id: TaskId,
    intent: PlanSessionIntent,
    pushed_prompt: Option<PushedPrompt>,
    mode: DispatchMode,
    directives: LaunchDirectives,
    agent: Agent,
    model_override: AgentModel,
    effort: SessionEffort,
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
/// Returns [`PlanSessionError`] for lookup, launch validation, or model selection failures.
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
            id: command.task_id.to_string(),
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
    let target = logic::dispatch_target(&command.task_id);
    let task_content = logic::load_task_content(command.task_id.as_ref(), store, pool).await?;
    let plan = SessionPlan {
        launch: AgentLaunch {
            agent: command.agent,
            task_id: command.task_id.to_string(),
            title: logic::thread_title(
                &task,
                &command.task_id,
                command.directives,
                command.agent,
                command.effort,
            ),
            repository: task.repo.clone().unwrap_or_default(),
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
    if !repository.is_directory(&plan.launch.repository) {
        return Err(PlanSessionError::RepositoryMissing {
            project: task.project.clone(),
            path: plan.launch.repository.clone(),
        });
    }
    if matches!(command.intent, PlanSessionIntent::Dispatch)
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

#[cfg(test)]
mod tests {
    use pwf_models::task::TaskId;

    use super::{
        Agent, AgentModel, DispatchMode, LaunchDirectives, PlanSession, PlanSessionIntent,
        SessionEffort,
    };

    struct SessionTaskId(TaskId);

    impl From<SessionTaskId> for TaskId {
        fn from(value: SessionTaskId) -> Self {
            value.0
        }
    }

    #[test]
    fn request_builder_maps_task_id_inputs() {
        let task_id = TaskId::try_new("PWF-0154").unwrap();
        let request = PlanSession::builder(SessionTaskId(task_id.clone()))
            .intent(PlanSessionIntent::DryRun)
            .mode(DispatchMode::Inline)
            .directives(LaunchDirectives::default())
            .agent(Agent::Codex)
            .model_override(AgentModel::default())
            .effort(SessionEffort::High)
            .build();

        assert_eq!(request.task_id, task_id);
    }
}
