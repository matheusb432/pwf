//! How an agent is run inside the new zellij tab. `AgentLauncher` is the seam;
//! each harness owns its argv in its own module (`claude`, `codex`), and the
//! argument-injection guard shared by both lives in `argv`. The tab name is
//! agent-independent, so it is not part of this trait — see `session::tab_name`.

mod argv;
mod claude;
mod codex;

pub(in crate::engines::pending_work) use claude::ClaudeLauncher;
pub(in crate::engines::pending_work) use codex::CodexLauncher;

use crate::engines::pending_work::{launch::Worktree, model::Item};

/// Builds the argv that runs an agent in the new zellij tab. The trait models only
/// what genuinely varies between harnesses — the binary and the argv shape.
pub(in crate::engines::pending_work) trait AgentLauncher {
    /// The agent binary — argv[0]. Single source for the binary name: the dispatch
    /// preflight warning, the `verify` availability line, and the PATH probe all read it.
    fn binary(&self) -> &str;
    /// argv after `zellij … new-tab … --`, e.g. `["claude","--name",<title>,"--",<prompt>]`.
    /// `worktree` augments the launch prompt with a git-worktree setup step.
    fn argv(&self, item: &Item, worktree: Worktree) -> Vec<String>;
}

/// Resolve the selected agent to its launcher. Both impls are unit structs, so the
/// reference is const-promoted to `'static` — no allocation, no boxing.
pub(in crate::engines::pending_work) fn launcher_for(
    agent: crate::cli::Agent,
) -> &'static dyn AgentLauncher {
    match agent {
        crate::cli::Agent::Claude => &ClaudeLauncher,
        crate::cli::Agent::Codex => &CodexLauncher,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launcher_for_maps_agent_to_binary() {
        assert_eq!(launcher_for(crate::cli::Agent::Claude).binary(), "claude");
        assert_eq!(launcher_for(crate::cli::Agent::Codex).binary(), "codex");
    }
}
