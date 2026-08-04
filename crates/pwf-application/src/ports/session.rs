#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentCommand<'a>(&'a [String]);

impl<'a> AgentCommand<'a> {
    #[must_use]
    pub fn new(arguments: &'a [String]) -> Self {
        Self(arguments)
    }

    #[must_use]
    pub fn arguments(&self) -> &'a [String] {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionStart<'a> {
    session_name: &'a str,
    working_directory: &'a str,
}

impl<'a> SessionStart<'a> {
    #[must_use]
    pub fn new(session_name: &'a str, working_directory: &'a str) -> Self {
        Self {
            session_name,
            working_directory,
        }
    }

    #[must_use]
    pub fn session_name(&self) -> &str {
        self.session_name
    }

    #[must_use]
    pub fn working_directory(&self) -> &str {
        self.working_directory
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionWindow<'a> {
    session_name: &'a str,
    working_directory: &'a str,
    window_name: &'a str,
    agent_command: AgentCommand<'a>,
}

impl<'a> SessionWindow<'a> {
    #[must_use]
    pub fn new(
        session_name: &'a str,
        working_directory: &'a str,
        window_name: &'a str,
        agent_command: AgentCommand<'a>,
    ) -> Self {
        Self {
            session_name,
            working_directory,
            window_name,
            agent_command,
        }
    }

    #[must_use]
    pub fn session_name(&self) -> &str {
        self.session_name
    }

    #[must_use]
    pub fn working_directory(&self) -> &str {
        self.working_directory
    }

    #[must_use]
    pub fn window_name(&self) -> &str {
        self.window_name
    }

    #[must_use]
    pub fn agent_command(&self) -> AgentCommand<'_> {
        self.agent_command
    }
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
