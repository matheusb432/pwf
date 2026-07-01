//! Dispatch outcome model and its colored Markdown rendering. The outcome enum
//! is rendered separately from coloring so both are unit-tested; ANSI is gated
//! by an explicit `on` bool resolved once at the edge (`color::use_color`).

use anstyle::AnsiColor;

use super::super::color::paint;

/// What a dispatch attempt produced.
#[derive(Debug, PartialEq, Eq)]
pub(in crate::engines::pending_work) enum DispatchOutcome {
    /// Session was already running; tab added cleanly.
    Success { session: String, tab: String },
    /// Session was dead → created via fallback → tab added.
    Fallback { session: String, tab: String },
    /// `new_tab` failed even after the fallback.
    Error {
        session: String,
        tab: String,
        message: String,
    },
}

/// Render the outcome: a heading with a branchable token, the bold+colored
/// session/tab line, and context. `agent`/`cwd` are echoed for the human.
pub(in crate::engines::pending_work) fn render(
    outcome: &DispatchOutcome,
    agent: &str,
    cwd: &str,
    on: bool,
) -> String {
    let (token, color, session, tab, note) = match outcome {
        DispatchOutcome::Success { session, tab } => {
            ("dispatched", AnsiColor::Green, session, tab, None)
        }
        DispatchOutcome::Fallback { session, tab } => (
            "created session + dispatched",
            AnsiColor::Yellow,
            session,
            tab,
            Some("note: the zellij session was not running — created it.".to_string()),
        ),
        DispatchOutcome::Error {
            session,
            tab,
            message,
        } => (
            "failed",
            AnsiColor::Red,
            session,
            tab,
            Some(format!("error: {message}")),
        ),
    };
    let line = paint(&format!("session: {session}  ·  tab: {tab}"), color, on);
    let mut out = format!("# session {tab} — {token}\n{line}\n");
    if !matches!(outcome, DispatchOutcome::Error { .. }) {
        out.push_str(&format!(
            "agent: {agent} · cwd: {cwd}\nattach with: zellij attach {session}\n"
        ));
    }
    if let Some(n) = note {
        out.push_str(&n);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_renders_green_token_and_bold_session_tab_plain() {
        let o = DispatchOutcome::Success {
            session: "cfg".into(),
            tab: "CFG-0009".into(),
        };
        let out = render(&o, "claude", "/repo", false);
        assert!(out.starts_with("# session CFG-0009 — dispatched"));
        assert!(out.contains("**session: cfg  ·  tab: CFG-0009**"));
        assert!(out.contains("attach with: zellij attach cfg"));
        assert!(!out.contains('\u{1b}')); // no ANSI when off
    }

    #[test]
    fn fallback_renders_created_token_and_note() {
        let o = DispatchOutcome::Fallback {
            session: "cfg".into(),
            tab: "CFG-0009".into(),
        };
        let out = render(&o, "claude", "/repo", false);
        assert!(out.starts_with("# session CFG-0009 — created session + dispatched"));
        assert!(out.contains("was not running"));
    }

    #[test]
    fn error_renders_failed_token_and_message() {
        let o = DispatchOutcome::Error {
            session: "cfg".into(),
            tab: "CFG-0009".into(),
            message: "boom".into(),
        };
        let out = render(&o, "claude", "/repo", false);
        assert!(out.starts_with("# session CFG-0009 — failed"));
        assert!(out.contains("error: boom"));
    }

    #[test]
    fn color_on_emits_ansi() {
        let o = DispatchOutcome::Success {
            session: "cfg".into(),
            tab: "CFG-0009".into(),
        };
        let out = render(&o, "claude", "/repo", true);
        assert!(out.contains('\u{1b}')); // ANSI present when on
    }
}
