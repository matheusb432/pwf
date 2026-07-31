//! Probes providers and prepares session launches and command previews.

mod argv;
mod claude;
mod codex;

use std::process::Command;

pub use pwf_application::pending_work::session::AgentProbe;
use pwf_application::{
    pending_work::session::{AgentLaunch, ModelTierLookup},
    ports::agent::{AgentClient, PreparedAgentLaunch},
};
use pwf_models::{pending_work::EffortTier, session::Agent};
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

fn probe(binary: &str) -> AgentProbe {
    let (available, path, version) = match which_binary(binary) {
        None => (false, None, None),
        Some(path) => {
            let version = run_version(&path);
            (true, Some(path), version)
        }
    };
    AgentProbe {
        binary: binary.to_string(),
        available,
        path,
        version,
    }
}

fn which_binary(name: &str) -> Option<String> {
    #[cfg(windows)]
    {
        let output = Command::new("where").arg(name).output().ok()?;
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            return stdout.lines().next().map(|line| line.trim().to_string());
        }
        None
    }
    #[cfg(not(windows))]
    {
        let output = Command::new("sh")
            .args(["-c", &format!("command -v {name}")])
            .output()
            .ok()?;
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let first = stdout.lines().next()?.trim();
            if !first.is_empty() {
                return Some(first.to_string());
            }
        }
        None
    }
}

fn run_version(path: &str) -> Option<String> {
    let output = Command::new(path).arg("--version").output().ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.lines().next().map(|line| line.trim().to_string())
}

/// Renders complete argv as a shell-safe command preview.
#[must_use]
pub fn render_argv(argv: &[String]) -> String {
    argv.iter()
        .map(|argument| shell_words::quote(argument))
        .collect::<Vec<_>>()
        .join(" ")
}
