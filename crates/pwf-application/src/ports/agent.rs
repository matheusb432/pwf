use std::error::Error;

use pwf_models::{session::Agent, task::EffortTier};

use crate::contract::task::session::{AgentLaunch, AgentProbe, ModelTierLookup};

pub trait AgentClient: Clone + Send + Sync + 'static {
    type ModelTierError: Error + Send + Sync + 'static;
    type PreparationError: Error + Send + Sync + 'static;

    #[must_use]
    fn probe(&self, agent: Agent) -> AgentProbe;

    fn model_tier(&self, effort: EffortTier) -> Result<ModelTierLookup, Self::ModelTierError>;

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
