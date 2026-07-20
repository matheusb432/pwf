use std::fmt::Write;

use anstyle::AnsiColor;
use pwf_application::pending_work::session::DispatchSessionOutcome;

use super::{agent_name, paint};

pub(in crate::engines::pending_work) fn render_dispatch(
    outcome: &DispatchSessionOutcome,
    on: bool,
) -> String {
    let (token, color, target, agent, repository, note) = match outcome {
        DispatchSessionOutcome::Aborted { task_id } => {
            return format!("# session {task_id} — aborted\nnothing dispatched.\n");
        }
        DispatchSessionOutcome::Inline { task_id } => {
            return format!("# session {task_id} — ran inline\n");
        }
        DispatchSessionOutcome::Direct {
            target,
            agent,
            repository,
        } => (
            "dispatched",
            AnsiColor::Green,
            target,
            Some(*agent),
            Some(repository),
            None,
        ),
        DispatchSessionOutcome::Recovered {
            target,
            agent,
            repository,
        } => (
            "created session + dispatched",
            AnsiColor::Yellow,
            target,
            Some(*agent),
            Some(repository),
            Some("note: the zellij session was not running — created it.".to_string()),
        ),
        DispatchSessionOutcome::Failed { target, message } => (
            "failed",
            AnsiColor::Red,
            target,
            None,
            None,
            Some(format!("error: {message}")),
        ),
    };
    let session = &target.session;
    let tab = &target.tab;
    let line = paint(&format!("session: {session}  ·  tab: {tab}"), color, on);
    let mut out = format!("# session {tab} — {token}\n{line}\n");
    if let (Some(agent), Some(repository)) = (agent, repository) {
        let _ = write!(
            out,
            "agent: {} · cwd: {repository}\nattach with: zellij attach {session}\n",
            agent_name(agent)
        );
    }
    if let Some(n) = note {
        out.push_str(&n);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use pwf_application::pending_work::session::{Agent, DispatchSessionOutcome, DispatchTarget};

    use super::*;

    fn target() -> DispatchTarget {
        DispatchTarget {
            session: "cfg".into(),
            tab: "CFG-0009".into(),
        }
    }

    #[test]
    fn success_renders_green_token_and_bold_session_tab_plain() {
        let outcome = DispatchSessionOutcome::Direct {
            target: target(),
            agent: Agent::Claude,
            repository: "/repo".into(),
        };
        let out = render_dispatch(&outcome, false);
        assert!(out.starts_with("# session CFG-0009 — dispatched"));
        assert!(out.contains("**session: cfg  ·  tab: CFG-0009**"));
        assert!(out.contains("attach with: zellij attach cfg"));
        assert!(!out.contains('\u{1b}'));
    }

    #[test]
    fn fallback_renders_created_token_and_note() {
        let outcome = DispatchSessionOutcome::Recovered {
            target: target(),
            agent: Agent::Claude,
            repository: "/repo".into(),
        };
        let out = render_dispatch(&outcome, false);
        assert!(out.starts_with("# session CFG-0009 — created session + dispatched"));
        assert!(out.contains("was not running"));
    }

    #[test]
    fn error_renders_failed_token_and_message() {
        let outcome = DispatchSessionOutcome::Failed {
            target: target(),
            message: "boom".into(),
        };
        let out = render_dispatch(&outcome, false);
        assert!(out.starts_with("# session CFG-0009 — failed"));
        assert!(out.contains("error: boom"));
    }

    #[test]
    fn color_on_emits_ansi() {
        let outcome = DispatchSessionOutcome::Direct {
            target: target(),
            agent: Agent::Claude,
            repository: "/repo".into(),
        };
        let out = render_dispatch(&outcome, true);
        assert!(out.contains('\u{1b}'));
    }

    #[test]
    fn aborted_and_inline_outcomes_preserve_their_compact_text() {
        assert_eq!(
            render_dispatch(
                &DispatchSessionOutcome::Aborted {
                    task_id: "PWF-0001".into()
                },
                false
            ),
            "# session PWF-0001 — aborted\nnothing dispatched.\n"
        );
        assert_eq!(
            render_dispatch(
                &DispatchSessionOutcome::Inline {
                    task_id: "PWF-0001".into()
                },
                false
            ),
            "# session PWF-0001 — ran inline\n"
        );
    }
}
