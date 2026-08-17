use std::fmt::Write;

use anstyle::AnsiColor;
use pwf_infra::session::render_argv;
use pwf_models::{
    session::{Agent, DispatchMode},
    task::TaskId,
};
use pwf_wire::task::session::{DispatchedSession, SessionPlan};

use super::{agent_name, paint};

pub(in crate::task) fn render_dispatch(outcome: &DispatchedSession, on: bool) -> String {
    let (target, agent, project_path) = match outcome {
        DispatchedSession::Inline { task_id } => {
            return format!("# session {task_id}: ran inline\n");
        }
        DispatchedSession::WindowOpened {
            target,
            agent,
            project_path,
        } => (target, *agent, project_path),
    };
    let session = target.session_name();
    let window = target.task_id();
    let line = paint(
        &format!("session: {session}  ·  window: {window}"),
        AnsiColor::Green,
        on,
    );
    let mut out = format!("# session {window}: dispatched\n{line}\n");
    let _ = write!(
        out,
        "agent: {} · cwd: {project_path}\n\
outside tmux: tmux attach-session -t ={session}\n\
inside tmux: tmux switch-client -t ={session}\n",
        agent_name(agent)
    );
    out
}

pub(in crate::task) fn render_session_aborted(task_id: &TaskId) -> String {
    format!("# session {task_id}: aborted\nnothing dispatched.\n")
}

pub(in crate::task) fn render_dry_run(plan: &SessionPlan, argv: &[String]) -> String {
    let agent = match plan.launch.agent {
        Agent::Claude => "Claude",
        Agent::Codex => "Codex",
    };
    let target = match plan.mode {
        DispatchMode::Inline => "inline".to_string(),
        DispatchMode::Multiplexer => {
            let target = plan.target();
            format!("tmux: {} / {}", target.session_name(), target.task_id())
        }
    };
    format!(
        "# session {} - dry run\n\
task: {}\n\
agent: {agent}\n\
model: {}\n\
effort: {}\n\
project_path: {}\n\
{target}\n\
command: {}\n\
nothing dispatched.\n",
        plan.launch.task_id,
        plan.launch.title,
        plan.launch.model,
        plan.launch.effort,
        plan.launch.project_path,
        render_argv(argv),
    )
}

#[cfg(test)]
mod tests {
    use pwf_models::{
        session::{
            Agent, AgentModel, DispatchMode, LaunchPrompt, SessionEffort, SessionThreadTitle,
            SessionWorkingDirectory,
        },
        task::TaskId,
    };
    use pwf_wire::task::session::{AgentLaunch, DispatchTarget, SessionPlan};

    use super::*;

    fn target() -> DispatchTarget {
        DispatchTarget::new(TaskId::try_new("AUX-0009").unwrap())
    }

    #[test]
    fn success_renders_green_token_and_bold_session_window_plain() {
        let outcome = DispatchedSession::WindowOpened {
            target: target(),
            agent: Agent::Claude,
            project_path: SessionWorkingDirectory::new("/project".to_string()),
        };
        let out = render_dispatch(&outcome, false);
        assert!(out.starts_with("# session AUX-0009: dispatched"));
        assert!(out.contains("**session: aux  ·  window: AUX-0009**"));
        assert!(out.contains("outside tmux: tmux attach-session -t =aux"));
        assert!(out.contains("inside tmux: tmux switch-client -t =aux"));
        assert!(!out.contains('\u{1b}'));
    }

    #[test]
    fn aborted_and_inline_results_preserve_their_compact_text() {
        assert_eq!(
            render_session_aborted(&TaskId::try_new("PWF-0001").unwrap()),
            "# session PWF-0001: aborted\nnothing dispatched.\n"
        );
        assert_eq!(
            render_dispatch(
                &DispatchedSession::Inline {
                    task_id: TaskId::try_new("PWF-0001").unwrap()
                },
                false
            ),
            "# session PWF-0001: ran inline\n"
        );
    }

    #[test]
    fn dry_run_renders_reasoning_effort() {
        let plan = SessionPlan {
            launch: AgentLaunch {
                agent: Agent::Codex,
                task_id: TaskId::try_new("PWF-0001").unwrap(),
                title: SessionThreadTitle::new("PWF-0001 - reason carefully".to_string()),
                project_path: SessionWorkingDirectory::new("/project".to_string()),
                prompt: LaunchPrompt::new("Inspect PWF-0001.".to_string()),
                model: AgentModel::default(),
                effort: SessionEffort::High,
            },
            mode: DispatchMode::Inline,
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
        assert!(out.contains("project_path: /project"));
    }
}
