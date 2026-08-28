use std::fmt::Write;

use anstyle::AnsiColor;
use pwf_client::{
    render_argv,
    v1::{
        Agent, DispatchMode, DispatchSessionOutcome, DispatchedSession, PlanSessionResponse,
        SessionEffort,
    },
};

use super::{agent_name, paint};

pub(in crate::task) fn render_dispatch(
    outcome: &DispatchedSession,
    on: bool,
) -> anyhow::Result<String> {
    match DispatchSessionOutcome::try_from(outcome.outcome).ok() {
        Some(DispatchSessionOutcome::Aborted) => Ok(render_session_aborted(&outcome.task_id)),
        Some(DispatchSessionOutcome::InlineLaunch) => {
            Ok(format!("# session {}: ran inline\n", outcome.task_id))
        }
        Some(DispatchSessionOutcome::WindowOpened) => {
            let opened = outcome.window_opened.as_ref().ok_or_else(|| {
                anyhow::anyhow!("pwf-server returned a window dispatch without a target")
            })?;
            let line = paint(
                &format!("session: {}  ·  window: {}", opened.session, opened.task_id),
                AnsiColor::Green,
                on,
            );
            let mut out = format!("# session {}: dispatched\n{line}\n", opened.task_id);
            let agent = Agent::try_from(opened.agent).unwrap_or(Agent::Unspecified);
            let _ = write!(
                out,
                "agent: {} · cwd: {}\n\
outside tmux: tmux attach-session -t ={}\n\
inside tmux: tmux switch-client -t ={}\n",
                agent_name(agent),
                opened.project_path,
                opened.session,
                opened.session,
            );
            Ok(out)
        }
        Some(DispatchSessionOutcome::Unspecified) | None => Err(anyhow::anyhow!(
            "pwf-server returned an invalid session outcome"
        )),
    }
}

pub(in crate::task) fn render_session_aborted(task_id: &str) -> String {
    format!("# session {task_id}: aborted\nnothing dispatched.\n")
}

pub(in crate::task) fn render_dry_run(outcome: &PlanSessionResponse) -> anyhow::Result<String> {
    let plan = outcome
        .plan
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("pwf-server returned a dry run without a plan"))?;
    let launch = plan
        .launch
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("pwf-server returned a dry run without a launch"))?;
    let agent = Agent::try_from(launch.agent).unwrap_or(Agent::Unspecified);
    let mode = DispatchMode::try_from(plan.mode).unwrap_or(DispatchMode::Unspecified);
    let target = match mode {
        DispatchMode::Inline => "inline".to_string(),
        DispatchMode::Multiplexer => format!(
            "tmux: {} / {}",
            session_name(&launch.task_id),
            launch.task_id
        ),
        DispatchMode::Unspecified => "unknown target".to_string(),
    };
    let effort = SessionEffort::try_from(launch.effort).unwrap_or(SessionEffort::Unspecified);
    Ok(format!(
        "# session {} - dry run\n\
task: {}\n\
agent: {}\n\
model: {}\n\
effort: {}\n\
project_path: {}\n\
{target}\n\
command: {}\n\
nothing dispatched.\n",
        launch.task_id,
        launch.title,
        title_agent_name(agent),
        launch.model.as_deref().unwrap_or("default"),
        effort_name(effort),
        launch.project_path,
        render_argv(&outcome.argv),
    ))
}

pub(super) fn session_name(task_id: &str) -> String {
    task_id
        .split_once('-')
        .map_or(task_id, |(project, _)| project)
        .to_ascii_lowercase()
}

pub(super) fn effort_name(effort: SessionEffort) -> &'static str {
    match effort {
        SessionEffort::Low => "low",
        SessionEffort::Medium => "medium",
        SessionEffort::High => "high",
        SessionEffort::Xhigh => "xhigh",
        SessionEffort::Max => "max",
        SessionEffort::Unspecified => "unspecified",
    }
}

fn title_agent_name(agent: Agent) -> &'static str {
    match agent {
        Agent::Claude => "Claude",
        Agent::Codex => "Codex",
        Agent::Unspecified => "Agent",
    }
}

#[cfg(test)]
mod tests {
    use pwf_client::v1::{AgentLaunch, InlineLaunch, SessionPlan, WindowOpened};

    use super::*;

    #[test]
    fn success_renders_green_token_and_bold_session_window_plain() {
        let outcome = DispatchedSession {
            outcome: DispatchSessionOutcome::WindowOpened as i32,
            task_id: "AUX-0009".to_string(),
            inline_launch: None,
            window_opened: Some(WindowOpened {
                task_id: "AUX-0009".to_string(),
                session: "aux".to_string(),
                agent: Agent::Claude as i32,
                project_path: "/project".to_string(),
            }),
        };
        let out = render_dispatch(&outcome, false).unwrap();
        assert!(out.starts_with("# session AUX-0009: dispatched"));
        assert!(out.contains("**session: aux  ·  window: AUX-0009**"));
        assert!(out.contains("outside tmux: tmux attach-session -t =aux"));
        assert!(out.contains("inside tmux: tmux switch-client -t =aux"));
        assert!(!out.contains('\u{1b}'));
    }

    #[test]
    fn aborted_and_inline_results_preserve_their_compact_text() {
        assert_eq!(
            render_session_aborted("FOO-0001"),
            "# session FOO-0001: aborted\nnothing dispatched.\n"
        );
        let outcome = DispatchedSession {
            outcome: DispatchSessionOutcome::InlineLaunch as i32,
            task_id: "FOO-0001".to_string(),
            inline_launch: Some(InlineLaunch {
                task_id: "FOO-0001".to_string(),
                argv: vec!["codex".to_string()],
                working_directory: "/project".to_string(),
            }),
            window_opened: None,
        };
        assert_eq!(
            render_dispatch(&outcome, false).unwrap(),
            "# session FOO-0001: ran inline\n"
        );
    }

    #[test]
    fn dry_run_renders_reasoning_effort() {
        let outcome = PlanSessionResponse {
            plan: Some(SessionPlan {
                launch: Some(AgentLaunch {
                    agent: Agent::Codex as i32,
                    task_id: "FOO-0001".to_string(),
                    title: "FOO-0001 - reason carefully".to_string(),
                    project_path: "/project".to_string(),
                    prompt: "Inspect FOO-0001.".to_string(),
                    model: None,
                    effort: SessionEffort::High as i32,
                }),
                mode: DispatchMode::Inline as i32,
            }),
            argv: vec![
                "codex".to_string(),
                "resume".to_string(),
                "-c".to_string(),
                "model_reasoning_effort=\"high\"".to_string(),
                "<thread-id returned by thread/start>".to_string(),
                "--".to_string(),
                "Inspect FOO-0001.".to_string(),
            ],
            probe: None,
            warnings: Vec::new(),
        };
        let out = render_dry_run(&outcome).unwrap();
        assert!(out.contains("effort: high"));
        assert!(out.contains("project_path: /project"));
    }
}
