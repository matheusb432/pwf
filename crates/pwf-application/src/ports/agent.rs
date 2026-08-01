use std::error::Error;

use pwf_models::{pending_work::EffortTier, session::Agent};

use crate::pending_work::session::{AgentLaunch, AgentProbe, ModelTierLookup};

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
