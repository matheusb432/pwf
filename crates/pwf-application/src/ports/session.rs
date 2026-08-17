use std::error::Error;

use pwf_models::session::SessionWorkingDirectory;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentCommand<'a> {
    program: &'a str,
    arguments: &'a [String],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("agent command must contain a program")]
pub struct EmptyAgentCommand;

impl<'a> AgentCommand<'a> {
    /// Separates a non-empty argument vector into its program and remaining arguments.
    ///
    /// # Errors
    ///
    /// Returns [`EmptyAgentCommand`] when `argv` has no program.
    pub fn try_new(argv: &'a [String]) -> Result<Self, EmptyAgentCommand> {
        let Some((program, arguments)) = argv.split_first() else {
            return Err(EmptyAgentCommand);
        };
        Ok(Self { program, arguments })
    }

    #[must_use]
    pub const fn program(self) -> &'a str {
        self.program
    }

    #[must_use]
    pub const fn arguments(self) -> &'a [String] {
        self.arguments
    }

    pub fn iter(self) -> impl Iterator<Item = &'a str> {
        std::iter::once(self.program).chain(self.arguments.iter().map(String::as_str))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionStart<'a> {
    pub session_name: &'a str,
    pub working_directory: &'a SessionWorkingDirectory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionWindow<'a> {
    pub session_name: &'a str,
    pub working_directory: &'a SessionWorkingDirectory,
    pub window_name: &'a str,
    pub agent_command: AgentCommand<'a>,
}

pub trait SessionClient: Clone + Send + Sync + 'static {
    type Error: Error + Send + Sync + 'static;

    #[must_use]
    fn available(&self) -> bool;

    fn session_exists(&self, session_name: &str) -> Result<bool, Self::Error>;

    #[must_use]
    fn preview_start(&self, start: &SessionStart<'_>) -> Vec<String>;

    #[must_use]
    fn preview_window(&self, window: &SessionWindow<'_>) -> Vec<String>;

    fn open_window(&self, window: &SessionWindow<'_>) -> Result<(), Self::Error>;
}

#[cfg(test)]
mod tests {
    use super::AgentCommand;

    #[test]
    fn agent_command_requires_at_least_the_program() {
        let empty = Vec::<String>::new();
        assert!(AgentCommand::try_new(&empty).is_err());

        let arguments = vec!["codex".to_string(), "--model".to_string()];
        let command = AgentCommand::try_new(&arguments).unwrap();
        assert_eq!(command.program(), "codex");
        assert_eq!(command.arguments(), &["--model"]);
        assert_eq!(command.iter().collect::<Vec<_>>(), ["codex", "--model"]);
    }
}
