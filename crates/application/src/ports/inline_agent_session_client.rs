use super::AgentCommand;

pub trait InlineAgentSessionClient: Clone + Send + Sync + 'static {
    /// Runs a prepared agent command in the supplied working directory.
    ///
    /// # Errors
    ///
    /// Returns a process error or unsuccessful exit status.
    fn run(&self, command: AgentCommand<'_>, working_directory: &str) -> Result<(), String>;
}
