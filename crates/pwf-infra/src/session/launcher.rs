//! Probes providers and prepares session launches and command previews.

mod argv;
mod claude;
mod codex;

use std::process::Command;

use pwf_application::ports::agent::{AgentClient, PreparedAgentLaunch};
use pwf_models::{session::Agent, task::EffortTier};
use pwf_wire::task::session::{AgentAvailability, AgentLaunch, AgentProbe, ModelTierLookup};
use thiserror::Error;

use super::{
    codex_app_server::CodexThreadPreparationError,
    model_tiers::{self, ModelTiersError},
};

#[derive(Debug, Error)]
#[error(transparent)]
pub struct AgentPreparationError(#[from] CodexThreadPreparationError);

/// Implements agent discovery, configuration, and launch preparation.
#[derive(Debug, Clone, Copy, Default)]
pub struct AgentHarness;

impl AgentClient for AgentHarness {
    type ModelTierError = ModelTiersError;
    type PreparationError = AgentPreparationError;

    fn probe(&self, agent: Agent) -> AgentProbe {
        match agent {
            Agent::Claude => claude::probe(),
            Agent::Codex => codex::probe(),
        }
    }

    fn model_tier(&self, effort: EffortTier) -> Result<ModelTierLookup, Self::ModelTierError> {
        model_tiers::tier(effort)
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
            Agent::Codex => codex::prepare(launch).map_err(Into::into),
        }
    }
}

fn probe(agent: Agent, binary: &str) -> AgentProbe {
    let availability = if binary_available(binary) {
        AgentAvailability::Available
    } else {
        AgentAvailability::Missing
    };
    AgentProbe {
        agent,
        availability,
    }
}

fn binary_available(name: &str) -> bool {
    #[cfg(windows)]
    {
        Command::new("where")
            .arg(name)
            .output()
            .is_ok_and(|output| output.status.success())
    }
    #[cfg(not(windows))]
    {
        Command::new("sh")
            .args(["-c", &format!("command -v {name}")])
            .output()
            .is_ok_and(|output| output.status.success())
    }
}

/// Renders complete argv as a shell-safe command preview.
#[must_use]
pub fn render_argv(argv: &[String]) -> String {
    argv.iter()
        .map(|argument| shell_words::quote(argument))
        .collect::<Vec<_>>()
        .join(" ")
}
