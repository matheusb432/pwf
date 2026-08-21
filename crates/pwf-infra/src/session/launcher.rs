//! Probes providers and prepares session launches and command previews.

mod argv;
mod claude;
mod codex;

use pwf_application::{
    contract::task::session::{AgentAvailability, AgentLaunch, AgentProbe, ModelTierLookup},
    ports::agent::{AgentClient, PreparedAgentLaunch},
};
use pwf_models::{session::Agent, task::EffortTier};
use thiserror::Error;

use super::{
    ProcessEnvironment,
    codex_app_server::CodexThreadPreparationError,
    model_tiers::{self, ModelTiersError},
};

#[derive(Debug, Error)]
#[error(transparent)]
pub struct AgentPreparationError(#[from] CodexThreadPreparationError);

/// Implements agent discovery, configuration, and launch preparation.
#[derive(Debug, Clone, Default)]
pub struct AgentHarness {
    environment: ProcessEnvironment,
}

impl AgentHarness {
    #[must_use]
    pub fn new(environment: ProcessEnvironment) -> Self {
        Self { environment }
    }
}

impl AgentClient for AgentHarness {
    type ModelTierError = ModelTiersError;
    type PreparationError = AgentPreparationError;

    fn probe(&self, agent: Agent) -> AgentProbe {
        match agent {
            Agent::Claude => claude::probe(&self.environment),
            Agent::Codex => codex::probe(&self.environment),
        }
    }

    fn model_tier(&self, effort: EffortTier) -> Result<ModelTierLookup, Self::ModelTierError> {
        model_tiers::tier(effort, &self.environment)
    }

    fn preview(&self, launch: &AgentLaunch) -> Vec<String> {
        match launch.agent {
            Agent::Claude => claude::preview(launch),
            Agent::Codex => codex::preview(launch),
        }
    }

    fn prepare(&self, launch: &AgentLaunch) -> Result<PreparedAgentLaunch, Self::PreparationError> {
        match launch.agent {
            Agent::Claude => Ok(PreparedAgentLaunch::Process {
                arguments: claude::prepare(launch),
            }),
            Agent::Codex => codex::prepare(launch, &self.environment).map_err(Into::into),
        }
    }
}

fn probe(environment: &ProcessEnvironment, agent: Agent, binary: &str) -> AgentProbe {
    let availability = if binary_available(environment, binary) {
        AgentAvailability::Available
    } else {
        AgentAvailability::Missing
    };
    AgentProbe {
        agent,
        availability,
    }
}

fn binary_available(environment: &ProcessEnvironment, name: &str) -> bool {
    #[cfg(windows)]
    {
        environment
            .command("where")
            .arg(name)
            .output()
            .is_ok_and(|output| output.status.success())
    }
    #[cfg(not(windows))]
    {
        environment
            .command("/bin/sh")
            .args(["-c", &format!("command -v {name}")])
            .output()
            .is_ok_and(|output| output.status.success())
    }
}
