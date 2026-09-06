//! Dispatches one confirmed agent session.

use pwf_wire::task::session::{DispatchedSession, PreparedSessionDispatch, SessionPlan};
use thiserror::Error;

use crate::ports::agent::{AgentClient, PreparedAgentLaunch};

#[derive(Debug, Error)]
pub enum DispatchSessionError {
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
) -> Result<DispatchedSession, DispatchSessionError> {
    let PreparedSessionDispatch { plan, .. } = prepared;

    let prepared = agent_client.prepare(&plan.launch).map_err(|source| {
        DispatchSessionError::AgentPreparation {
            source: anyhow::Error::new(source),
        }
    })?;
    match prepared {
        PreparedAgentLaunch::Process { arguments } => dispatch_inline(&arguments, &plan),
        PreparedAgentLaunch::NamedThread {
            arguments,
            thread_id,
        } => dispatch_inline(&arguments, &plan).map_err(|source| {
            DispatchSessionError::NamedThreadBackend {
                thread_id,
                source: Box::new(source),
            }
        }),
    }
}

fn dispatch_inline(
    argv: &[String],
    plan: &SessionPlan,
) -> Result<DispatchedSession, DispatchSessionError> {
    if argv.is_empty() {
        return Err(DispatchSessionError::EmptyAgentCommand);
    }
    Ok(DispatchedSession::InlineLaunch {
        task_ids: plan.launch.task_ids.clone(),
        argv: argv.to_vec(),
        working_directory: plan.launch.project_path.clone(),
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
        let dispatch_error = match error {
            DispatchSessionError::NamedThreadBackend { source, .. } => Some(source),
            _ => None,
        };
        assert!(dispatch_error.is_some());
        let source = match *dispatch_error.unwrap() {
            DispatchSessionError::AgentPreparation { source } => Some(source),
            _ => None,
        };
        assert!(source.is_some());
        let source = source.unwrap();
        assert!(source.downcast_ref::<SentinelError>().is_some());
        assert_eq!(
            source.root_cause().to_string(),
            "inline process unavailable"
        );
    }
}
