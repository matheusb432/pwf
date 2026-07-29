use std::fmt::Write;

use anstyle::AnsiColor;
use pwf_application::pending_work::session::{SessionPlan, dispatch_session::DispatchSessionOk};
use pwf_infra::session::render_argv;
use pwf_models::session::{Agent, DispatchMode};

use super::{agent_name, paint};

pub(in crate::pending_work) fn render_dispatch(outcome: &DispatchSessionOk, on: bool) -> String {
    let (token, color, target, agent, repository) = match outcome {
        DispatchSessionOk::Inline { task_id } => {
            return format!("# session {task_id} — ran inline\n");
        }
        DispatchSessionOk::WindowOpened {
            target,
            agent,
            repository,
        } => (
            "dispatched",
            AnsiColor::Green,
            target,
            Some(*agent),
            Some(repository),
        ),
    };
    let session = &target.session;
    let window = &target.window;
    let line = paint(
        &format!("session: {session}  ·  window: {window}"),
        color,
        on,
    );
    let mut out = format!("# session {window} — {token}\n{line}\n");
    if let (Some(agent), Some(repository)) = (agent, repository) {
        let _ = write!(
            out,
            "agent: {} · cwd: {repository}\n\
outside tmux: tmux attach-session -t ={session}\n\
inside tmux: tmux switch-client -t ={session}\n",
            agent_name(agent)
        );
    }
    out
}

pub(in crate::pending_work) fn render_session_aborted(task_id: &str) -> String {
    format!("# session {task_id} — aborted\nnothing dispatched.\n")
}

pub(in crate::pending_work) fn render_dry_run(plan: &SessionPlan, argv: &[String]) -> String {
    let agent = match plan.launch.agent {
        Agent::Claude => "Claude",
        Agent::Codex => "Codex",
    };
    let target = match plan.mode {
        DispatchMode::Inline => "inline".to_string(),
        DispatchMode::Multiplexer => {
            format!("tmux: {} / {}", plan.target.session, plan.target.window)
        }
    };
    format!(
        "# session {} - dry run\n\
task: {}\n\
agent: {agent}\n\
model: {}\n\
effort: {}\n\
repository: {}\n\
{target}\n\
command: {}\n\
nothing dispatched.\n",
        plan.launch.task_id,
        plan.launch.title,
        plan.launch.model.as_deref().unwrap_or("default"),
        plan.launch.effort,
        plan.launch.repository,
        render_argv(argv),
    )
}

#[cfg(test)]
mod tests {
    use pwf_application::pending_work::session::{
        AgentLaunch, DispatchTarget, SessionPlan, dispatch_session::DispatchSessionOk,
    };
    use pwf_models::session::{Agent, DispatchMode, SessionEffort};

    use super::*;

    fn target() -> DispatchTarget {
        DispatchTarget {
            session: "cfg".into(),
            window: "CFG-0009".into(),
        }
    }

    #[test]
    fn success_renders_green_token_and_bold_session_window_plain() {
        let outcome = DispatchSessionOk::WindowOpened {
            target: target(),
            agent: Agent::Claude,
            repository: "/repo".into(),
        };
        let out = render_dispatch(&outcome, false);
        assert!(out.starts_with("# session CFG-0009 — dispatched"));
        assert!(out.contains("**session: cfg  ·  window: CFG-0009**"));
        assert!(out.contains("outside tmux: tmux attach-session -t =cfg"));
        assert!(out.contains("inside tmux: tmux switch-client -t =cfg"));
        assert!(!out.contains('\u{1b}'));
    }

    #[test]
    fn color_on_emits_ansi() {
        let outcome = DispatchSessionOk::WindowOpened {
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

    #[test]
    fn dry_run_renders_reasoning_effort() {
        let plan = SessionPlan {
            launch: AgentLaunch {
                agent: Agent::Codex,
                task_id: "PWF-0001".into(),
                title: "PWF-0001 - reason carefully".into(),
                repository: "/repo".into(),
                prompt: "Inspect PWF-0001.".into(),
                model: None,
                effort: SessionEffort::High,
            },
            mode: DispatchMode::Inline,
            target: target(),
        };

        let out = render_dry_run(
            &plan,
            &[
                "codex".into(),
                "resume".into(),
                "-c".into(),
                "model_reasoning_effort=\"high\"".into(),
                "<thread-id returned by thread/start>".into(),
                "--".into(),
                "Inspect PWF-0001.".into(),
            ],
        );

        assert!(out.contains("effort: high"));
    }
}
