use std::error::Error;

use crate::pending_work::session::{AgentLaunch, AgentProbe};

pub trait CodexAgentSessionClient: Clone + Send + Sync + 'static {
    type Error: Error + Send + Sync + 'static;

    fn probe(&self) -> AgentProbe;

    fn preview(&self, launch: &AgentLaunch) -> Vec<String>;

    fn prepare(&self, launch: &AgentLaunch) -> Result<PreparedCodexLaunch, Self::Error>;
}

pub struct PreparedCodexLaunch {
    thread_id: String,
    argv: Vec<String>,
}

impl PreparedCodexLaunch {
    #[must_use]
    pub fn new(thread_id: String, argv: Vec<String>) -> Self {
        Self { thread_id, argv }
    }

    #[must_use]
    pub fn thread_id(&self) -> &str {
        &self.thread_id
    }

    #[must_use]
    pub fn argv(&self) -> &[String] {
        &self.argv
    }
}
