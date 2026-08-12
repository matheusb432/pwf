use nutype::nutype;
use pwf_models::session::SessionWorkingDirectory;

#[nutype(
    validate(predicate = |arguments| !arguments.is_empty()),
    derive(Debug, Clone, Copy, PartialEq, Eq, Deref),
)]
pub struct AgentCommand<'a>(&'a [String]);

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
    #[must_use]
    fn available(&self) -> bool;

    fn session_exists(&self, session_name: &str) -> Result<bool, String>;

    #[must_use]
    fn preview_start(&self, start: &SessionStart<'_>) -> Vec<String>;

    #[must_use]
    fn preview_window(&self, window: &SessionWindow<'_>) -> Vec<String>;

    fn open_window(&self, window: &SessionWindow<'_>) -> Result<(), String>;
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
        assert_eq!(*command, arguments.as_slice());
    }
}
