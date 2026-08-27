//! Dispatches one confirmed task session.

use pwf_models::task::TaskId;
use pwf_wire::task::session::{DispatchedSession, PreparedSessionDispatch, SessionPlan};
use thiserror::Error;

use super::DispatchMode;
use crate::ports::{
    agent::{AgentClient, PreparedAgentLaunch},
    session::{AgentCommand, SessionClient, SessionWindow},
};

#[derive(Debug, Error)]
pub enum DispatchSessionError {
    #[error("Failed to open multiplexer window '{window}' in session '{session}': {source}")]
    WindowOpen {
        session: String,
        window: TaskId,
        #[source]
        source: anyhow::Error,
    },
    #[error(transparent)]
    AgentPreparation { source: anyhow::Error },
    #[error(
        "Agent backend failed after naming thread '{thread_id}': {source}. The named thread was left intact."
    )]
    NamedThreadBackend {
        thread_id: String,
        #[source]
        source: Box<Self>,
    },
    #[error("Agent command is empty.")]
    EmptyAgentCommand,
}

#[cqrsy::command]
pub fn execute(
    prepared: PreparedSessionDispatch,
    agent_client: &impl AgentClient,
    session_client: &impl SessionClient,
) -> Result<DispatchedSession, DispatchSessionError> {
    let PreparedSessionDispatch { plan, .. } = prepared;

    let prepared = agent_client.prepare(&plan.launch).map_err(|source| {
        DispatchSessionError::AgentPreparation {
            source: anyhow::Error::new(source),
        }
    })?;
    match prepared {
        PreparedAgentLaunch::Process { arguments } => {
            dispatch_host(&arguments, &plan, session_client)
        }
        PreparedAgentLaunch::NamedThread {
            arguments,
            thread_id,
        } => dispatch_host(&arguments, &plan, session_client).map_err(|source| {
            DispatchSessionError::NamedThreadBackend {
                thread_id,
                source: Box::new(source),
            }
        }),
    }
}

fn dispatch_host(
    argv: &[String],
    plan: &SessionPlan,
    session_client: &impl SessionClient,
) -> Result<DispatchedSession, DispatchSessionError> {
    match plan.mode {
        DispatchMode::Inline => {
            AgentCommand::try_new(argv).map_err(|_| DispatchSessionError::EmptyAgentCommand)?;
            Ok(DispatchedSession::InlineLaunch {
                task_id: plan.launch.task_id.clone(),
                argv: argv.to_vec(),
                working_directory: plan.launch.project_path.clone(),
            })
        }
        DispatchMode::Multiplexer => dispatch_multiplexer(argv, plan, session_client),
    }
}

fn dispatch_multiplexer(
    argv: &[String],
    plan: &SessionPlan,
    session_client: &impl SessionClient,
) -> Result<DispatchedSession, DispatchSessionError> {
    let target = plan.target();
    let session_name = target.session_name();
    let agent_command =
        AgentCommand::try_new(argv).map_err(|_| DispatchSessionError::EmptyAgentCommand)?;
    let window = SessionWindow {
        session_name: &session_name,
        working_directory: &plan.launch.project_path,
        window_name: target.task_id().as_ref(),
        agent_command,
    };
    session_client
        .open_window(&window)
        .map_err(|source| DispatchSessionError::WindowOpen {
            session: session_name,
            window: target.task_id().clone(),
            source: anyhow::Error::new(source),
        })?;
    Ok(DispatchedSession::WindowOpened {
        target,
        agent: plan.launch.agent,
        project_path: plan.launch.project_path.clone(),
    })
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use super::DispatchSessionError;

    #[derive(Debug, thiserror::Error)]
    #[error("inline process unavailable")]
    struct SentinelError;

    #[test]
    fn named_thread_failures_retain_the_typed_dispatch_and_root_errors() {
        let error = DispatchSessionError::NamedThreadBackend {
            thread_id: "thread-42".to_string(),
            source: Box::new(DispatchSessionError::AgentPreparation {
                source: anyhow::Error::new(SentinelError),
            }),
        };

        assert_eq!(
            error.source().unwrap().to_string(),
            "inline process unavailable"
        );
        let DispatchSessionError::NamedThreadBackend {
            source: dispatch_error,
            ..
        } = error
        else {
            panic!("expected the named thread error");
        };
        let DispatchSessionError::AgentPreparation { source } = *dispatch_error else {
            panic!("expected the agent preparation error");
        };
        assert!(source.downcast_ref::<SentinelError>().is_some());
        assert_eq!(
            source.root_cause().to_string(),
            "inline process unavailable"
        );
    }
}
