mod close;
mod confirmation;
mod diagnostics;
mod list;
mod session;
mod session_confirmation;
mod task_summary;

use anstyle::Ansi256Color;
pub(super) use close::{emit_close_diagnostics, render_closed, render_reopened};
pub(super) use confirmation::{render_added, render_edited, render_removed};
pub(super) use diagnostics::{
    TITLE_NORMALIZED_NOTICE, emit_created_section, emit_created_section_for_error,
};
pub(super) use list::render_list;
pub(super) use session::{render_dispatch, render_dry_run};
pub(super) use session_confirmation::render_session_confirmation;
pub(super) use task_summary::{render_status, render_task_summary};

fn agent_name(agent: pwf_client::v1::Agent) -> &'static str {
    match agent {
        pwf_client::v1::Agent::Claude => "claude",
        pwf_client::v1::Agent::Codex => "codex",
        pwf_client::v1::Agent::Unspecified => "agent",
    }
}

const ID_ORANGE: Ansi256Color = Ansi256Color(208);

fn paint(text: &str, color: impl Into<anstyle::Color>, enabled: bool) -> String {
    if !enabled {
        return format!("**{text}**");
    }
    let style = anstyle::Style::new().bold().fg_color(Some(color.into()));
    format!("{}{}{}", style.render(), text, style.render_reset())
}
