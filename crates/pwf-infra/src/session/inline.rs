//! Executes prepared agent commands in the current terminal.

use std::{io, process::Command};

use pwf_application::ports::{
    inline_agent_session::InlineAgentSessionClient, session::AgentCommand,
};
use pwf_models::session::SessionWorkingDirectory;

/// Executes prepared agent commands inline.
#[derive(Debug, Clone, Copy, Default)]
pub struct InlineHarness;

#[derive(Debug, thiserror::Error)]
pub enum InlineSessionError {
    #[error("{source}")]
    Execute {
        #[source]
        source: io::Error,
    },
    #[error("agent exited with status {status}")]
    AgentExited { status: i32 },
}

impl InlineAgentSessionClient for InlineHarness {
    type Error = InlineSessionError;

    fn run(
        &self,
        command: AgentCommand<'_>,
        working_directory: &SessionWorkingDirectory,
    ) -> Result<(), Self::Error> {
        let mut process = Command::new(command.program());
        process
            .args(command.arguments())
            .current_dir(working_directory.as_ref());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            Err(InlineSessionError::Execute {
                source: process.exec(),
            })
        }
        #[cfg(not(unix))]
        {
            let status = process
                .status()
                .map_err(|source| InlineSessionError::Execute { source })?;
            map_spawn_status(status.success(), status.code())
        }
    }
}

#[cfg(any(not(unix), test))]
fn map_spawn_status(success: bool, code: Option<i32>) -> Result<(), InlineSessionError> {
    if success {
        Ok(())
    } else {
        Err(InlineSessionError::AgentExited {
            status: code.unwrap_or(-1),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{InlineSessionError, map_spawn_status};

    #[test]
    fn unsuccessful_status_retains_the_exit_classification() {
        let error = map_spawn_status(false, Some(17)).unwrap_err();
        assert!(matches!(
            error,
            InlineSessionError::AgentExited { status: 17 }
        ));
        assert_eq!(error.to_string(), "agent exited with status 17");

        let error = map_spawn_status(false, None).unwrap_err();
        assert!(matches!(
            error,
            InlineSessionError::AgentExited { status: -1 }
        ));
        assert_eq!(error.to_string(), "agent exited with status -1");
    }
}
