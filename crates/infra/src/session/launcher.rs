//! Encodes prepared semantic launches for each supported agent CLI.

mod argv;
mod claude;
pub mod codex;

use pwf_application::pending_work::session::{Agent, AgentLaunch};

const CLAUDE_BINARY: &str = "claude";

pub(super) fn binary(agent: Agent) -> &'static str {
    match agent {
        Agent::Claude => CLAUDE_BINARY,
        Agent::Codex => codex::BINARY,
    }
}

pub(super) fn launch_argv(launch: &AgentLaunch) -> Vec<String> {
    match launch.agent {
        Agent::Claude => claude::launch_argv(launch),
        Agent::Codex => codex::launch_argv(launch),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_maps_to_its_owned_binary() {
        assert_eq!(binary(Agent::Claude), "claude");
        assert_eq!(binary(Agent::Codex), "codex");
    }
}
