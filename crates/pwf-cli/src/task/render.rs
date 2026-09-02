mod close;
mod confirmation;
mod diagnostics;
mod list;
mod session;
mod session_confirmation;
mod task_summary;

pub(super) use close::{render_cancelled, render_completed, render_reopened};
pub(super) use confirmation::{render_added, render_edited, render_removed};
pub(super) use diagnostics::TITLE_NORMALIZED_NOTICE;
pub(super) use list::render_list;
pub(super) use session::{render_dispatch, render_dry_run};
pub(super) use session_confirmation::render_session_confirmation;
pub(super) use task_summary::{render_domain_task_identifier, render_status, render_task_summary};

fn agent_name(agent: pwf_client::pb::Agent) -> &'static str {
    match agent {
        pwf_client::pb::Agent::Claude => "claude",
        pwf_client::pb::Agent::Codex => "codex",
        pwf_client::pb::Agent::Unspecified => "agent",
    }
}
