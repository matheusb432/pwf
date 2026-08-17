use std::error::Error;

use pwf_models::session::SessionWorkingDirectory;

use super::session::AgentCommand;

pub trait InlineAgentSessionClient: Clone + Send + Sync + 'static {
    type Error: Error + Send + Sync + 'static;

    /// Runs a prepared agent command in the supplied working directory.
    ///
    /// # Errors
    ///
    /// Returns a process error or unsuccessful exit status.
    fn run(
        &self,
        command: AgentCommand<'_>,
        working_directory: &SessionWorkingDirectory,
    ) -> Result<(), Self::Error>;
}
