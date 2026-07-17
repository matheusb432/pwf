//! Presents `pwf session` confirmations, interaction effects, and dispatch outcomes.

mod confirmation;
mod interaction;
mod render;

pub(super) use interaction::CliSessionInteraction;
use pwf_application::pending_work::session::Agent;
pub(super) use render::render_dispatch;

fn agent_name(agent: Agent) -> &'static str {
    match agent {
        Agent::Claude => "claude",
        Agent::Codex => "codex",
    }
}
