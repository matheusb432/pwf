use std::fmt::Write;

use anstyle::AnsiColor;
use pwf_application::pending_work::session::{
    Agent, DispatchMode, SessionPlan, dispatch_session::DispatchSessionOk,
};
use pwf_infra::session::render_argv;

use super::{agent_name, paint};

pub(in crate::engines::pending_work) fn render_dispatch(
    outcome: &DispatchSessionOk,
    on: bool,
) -> String {
    let (token, color, target, agent, repository, note) = match outcome {
        DispatchSessionOk::Inline { task_id } => {
            return format!("# session {task_id} — ran inline\n");
        }
        DispatchSessionOk::TabOpened {
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
        DispatchSessionOk::MultiplexerStartedAndTabOpened {
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

pub(in crate::engines::pending_work) fn render_session_aborted(task_id: &str) -> String {
    format!("# session {task_id} — aborted\nnothing dispatched.\n")
}

pub(in crate::engines::pending_work) fn render_dry_run(
    plan: &SessionPlan,
    argv: &[String],
) -> String {
    let agent = match plan.launch.agent {
        Agent::Claude => "Claude",
        Agent::Codex => "Codex",
    };
    let target = match plan.mode {
        DispatchMode::Inline => "inline".to_string(),
        DispatchMode::Multiplexer => {
            format!("zellij: {} / {}", plan.target.session, plan.target.tab)
        }
    };
    format!(
        "# session {} - dry run\n\
task: {}\n\
agent: {agent}\n\
model: {}\n\
repository: {}\n\
{target}\n\
command: {}\n\
nothing dispatched.\n",
        plan.launch.task_id,
        plan.launch.title,
        plan.launch.model.as_deref().unwrap_or("default"),
        plan.launch.repository,
        render_argv(argv),
    )
}

#[cfg(test)]
mod tests {
    use pwf_application::pending_work::session::{
        Agent, DispatchTarget, dispatch_session::DispatchSessionOk,
    };

    use super::*;

    fn target() -> DispatchTarget {
        DispatchTarget {
            session: "cfg".into(),
            tab: "CFG-0009".into(),
        }
    }

    #[test]
    fn success_renders_green_token_and_bold_session_tab_plain() {
        let outcome = DispatchSessionOk::TabOpened {
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
        let outcome = DispatchSessionOk::MultiplexerStartedAndTabOpened {
            target: target(),
            agent: Agent::Claude,
            repository: "/repo".into(),
        };
        let out = render_dispatch(&outcome, false);
        assert!(out.starts_with("# session CFG-0009 — created session + dispatched"));
        assert!(out.contains("was not running"));
    }

    #[test]
    fn color_on_emits_ansi() {
        let outcome = DispatchSessionOk::TabOpened {
            target: target(),
            agent: Agent::Claude,
            repository: "/repo".into(),
        };
        let out = render_dispatch(&outcome, true);
        assert!(out.contains('\u{1b}'));
    }

    #[test]
    fn aborted_and_inline_results_preserve_their_compact_text() {
        assert_eq!(
            render_session_aborted("PWF-0001"),
            "# session PWF-0001 — aborted\nnothing dispatched.\n"
        );
        assert_eq!(
            render_dispatch(
                &DispatchSessionOk::Inline {
                    task_id: "PWF-0001".into()
                },
                false
            ),
            "# session PWF-0001 — ran inline\n"
        );
    }
}
