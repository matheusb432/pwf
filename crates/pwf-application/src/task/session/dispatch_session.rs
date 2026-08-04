//! Dispatches one confirmed task session.

use std::error::Error;

use pwf_models::task::TaskId;
use pwf_wire::task::session::{DispatchTarget, SessionPlan};
use thiserror::Error;

use super::{Agent, DispatchMode, plan_session::PreparedSessionDispatch};
use crate::ports::{
    agent::{AgentClient, PreparedAgentLaunch},
    inline_agent_session::InlineAgentSessionClient,
    session::{AgentCommand, SessionClient, SessionWindow},
};

pub struct DispatchSession {
    pub prepared: PreparedSessionDispatch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchSessionOk {
    Inline {
        task_id: TaskId,
    },
    WindowOpened {
        target: DispatchTarget,
        agent: Agent,
        project_path: String,
    },
}

#[derive(Debug, Error)]
pub enum DispatchSessionError {
    #[error("Failed to run agent inline: {message}")]
    InlineFailed { message: String },
    #[error("Failed to open multiplexer window '{window}' in session '{session}': {message}")]
    WindowOpen {
        session: String,
        window: TaskId,
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
    #[error("Agent command is empty.")]
    EmptyAgentCommand,
}

#[cqrsy::command]
pub fn execute(
    command: DispatchSession,
    agent_client: &impl AgentClient,
    inline: &impl InlineAgentSessionClient,
    session_client: &impl SessionClient,
) -> Result<DispatchSessionOk, DispatchSessionError> {
    let PreparedSessionDispatch { plan, .. } = command.prepared;

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
    plan: &SessionPlan,
    inline: &impl InlineAgentSessionClient,
    session_client: &impl SessionClient,
) -> Result<DispatchSessionOk, DispatchSessionError> {
    match plan.mode {
        DispatchMode::Inline => {
            let command =
                AgentCommand::try_new(argv).map_err(|_| DispatchSessionError::EmptyAgentCommand)?;
            inline
                .run(command, &plan.launch.project_path)
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
    plan: &SessionPlan,
    session_client: &impl SessionClient,
) -> Result<DispatchSessionOk, DispatchSessionError> {
    let session_name = plan.target.session_name();
    let agent_command =
        AgentCommand::try_new(argv).map_err(|_| DispatchSessionError::EmptyAgentCommand)?;
    let window = SessionWindow {
        session_name: &session_name,
        working_directory: &plan.launch.project_path,
        window_name: plan.target.task_id.as_ref(),
        agent_command,
    };
    session_client
        .open_window(&window)
        .map_err(|message| DispatchSessionError::WindowOpen {
            session: session_name,
            window: plan.target.task_id.clone(),
            message,
        })?;
    Ok(DispatchSessionOk::WindowOpened {
        target: plan.target.clone(),
        agent: plan.launch.agent,
        project_path: plan.launch.project_path.clone(),
    })
}
