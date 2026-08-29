use std::error::Error;

use pwf_models::session::Agent;
use pwf_wire::task::session::{AgentLaunch, AgentProbe};

pub trait AgentClient: Clone + Send + Sync + 'static {
    type PreparationError: Error + Send + Sync + 'static;

    #[must_use]
    fn probe(&self, agent: Agent) -> AgentProbe;

    #[must_use]
    fn preview(&self, launch: &AgentLaunch) -> Vec<String>;

    fn prepare(&self, launch: &AgentLaunch) -> Result<PreparedAgentLaunch, Self::PreparationError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreparedAgentLaunch {
    Process {
        arguments: Vec<String>,
    },
    NamedThread {
        arguments: Vec<String>,
        thread_id: String,
    },
}
