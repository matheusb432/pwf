mod close;
mod confirmation;
mod diagnostics;
mod list;
mod session;
mod session_confirmation;
mod verify;

use anstyle::Ansi256Color;
pub(super) use close::{emit_close_diagnostics, render_closed, render_reopened};
pub(super) use confirmation::{render_added, render_removed, render_updated};
pub(super) use diagnostics::{
    TITLE_NORMALIZED_NOTICE, emit_created_section, emit_created_section_for_error,
};
pub(super) use list::render_list;
pub(super) use session::{render_dispatch, render_dry_run, render_session_aborted};
pub(super) use session_confirmation::render_session_confirmation;
pub(super) use verify::render_verify;

fn agent_name(agent: pwf_application::pending_work::session::Agent) -> &'static str {
    match agent {
        pwf_application::pending_work::session::Agent::Claude => "claude",
        pwf_application::pending_work::session::Agent::Codex => "codex",
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
