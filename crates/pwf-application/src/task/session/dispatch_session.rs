//! Dispatches one confirmed task session.

use std::error::Error;

use thiserror::Error;

use super::{Agent, DispatchMode, DispatchTarget, logic, plan_session::PreparedSessionDispatch};
use crate::{
    ports::{
        agent::{AgentClient, PreparedAgentLaunch},
        inline_agent_session::InlineAgentSessionClient,
        project_note::ProjectNoteStore,
        session::{AgentCommand, SessionClient, SessionWindow},
        task_record::TaskStore,
    },
    task::{logic::task_update, show_task::ShowTaskError, update_task::UpdateTaskError},
};

pub struct DispatchSession {
    prepared: PreparedSessionDispatch,
}

impl DispatchSession {
    #[must_use]
    pub fn new(prepared: PreparedSessionDispatch) -> Self {
        Self { prepared }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchSessionOk {
    Inline {
        task_id: String,
    },
    WindowOpened {
        target: DispatchTarget,
        agent: Agent,
        repository: String,
    },
}

#[derive(Debug, Error)]
pub enum DispatchSessionError {
    #[error(transparent)]
    Update(#[from] UpdateTaskError),
    #[error(transparent)]
    Show(#[from] ShowTaskError),
    #[error("Failed to run agent inline: {message}")]
    InlineFailed { message: String },
    #[error("Failed to open multiplexer window '{window}' in session '{session}': {message}")]
    WindowOpen {
        session: String,
        window: String,
        message: String,
    },
    #[error("{source}")]
    AgentPreparation {
        #[source]
        source: Box<dyn Error + Send + Sync>,
    },
    #[error(
        "Agent backend failed after naming thread '{thread_id}': {message}. The named thread was left intact."
    )]
    NamedThreadBackend { thread_id: String, message: String },
}

#[cqrsy::command]
pub async fn execute(
    command: DispatchSession,
    store: &(impl TaskStore + ProjectNoteStore),
    pool: &sqlx::SqlitePool,
    agent_client: &impl AgentClient,
    inline: &impl InlineAgentSessionClient,
    session_client: &impl SessionClient,
) -> Result<DispatchSessionOk, DispatchSessionError> {
    let PreparedSessionDispatch {
        mut plan,
        confirmation,
        prepared_update,
        ..
    } = command.prepared;
    if let Some(prepared_update) = prepared_update {
        task_update::persist(prepared_update, store)?;
        let task_content = logic::load_task_content(&plan.launch.task_id, store, pool).await?;
        plan.launch.prompt =
            logic::launch_prompt(&task_content, &plan.launch.task_id, confirmation.directives);
    }

    let prepared = agent_client.prepare(&plan.launch).map_err(|source| {
        DispatchSessionError::AgentPreparation {
            source: Box::new(source),
        }
    })?;
    match prepared {
        PreparedAgentLaunch::Process { arguments } => {
            dispatch_host(&arguments, &plan, inline, session_client)
        }
        PreparedAgentLaunch::NamedThread {
            arguments,
            thread_id,
        } => dispatch_host(&arguments, &plan, inline, session_client).map_err(|error| {
            DispatchSessionError::NamedThreadBackend {
                thread_id,
                message: error.to_string(),
            }
        }),
    }
}

fn dispatch_host(
    argv: &[String],
    plan: &super::SessionPlan,
    inline: &impl InlineAgentSessionClient,
    session_client: &impl SessionClient,
) -> Result<DispatchSessionOk, DispatchSessionError> {
    match plan.mode {
        DispatchMode::Inline => {
            inline
                .run(AgentCommand::new(argv), &plan.launch.repository)
                .map_err(|message| DispatchSessionError::InlineFailed { message })?;
            Ok(DispatchSessionOk::Inline {
                task_id: plan.launch.task_id.clone(),
            })
        }
        DispatchMode::Multiplexer => dispatch_multiplexer(argv, plan, session_client),
    }
}

fn dispatch_multiplexer(
    argv: &[String],
    plan: &super::SessionPlan,
    session_client: &impl SessionClient,
) -> Result<DispatchSessionOk, DispatchSessionError> {
    let window = SessionWindow::builder()
        .session_name(&plan.target.session)
        .working_directory(&plan.launch.repository)
        .window_name(&plan.target.window)
        .agent_command(AgentCommand::new(argv))
        .build();
    session_client
        .open_window(&window)
        .map_err(|message| DispatchSessionError::WindowOpen {
            session: plan.target.session.clone(),
            window: plan.target.window.clone(),
            message,
        })?;
    Ok(DispatchSessionOk::WindowOpened {
        target: plan.target.clone(),
        agent: plan.launch.agent,
        repository: plan.launch.repository.clone(),
    })
}
