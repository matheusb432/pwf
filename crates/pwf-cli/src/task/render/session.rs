use std::fmt::Write;

use anstyle::AnsiColor;
use pwf_client::{
    pb::{
        Agent, DispatchMode, DispatchedSession, PlanSessionResponse, SessionEffort,
        dispatched_session,
    },
    render_argv,
};

use super::agent_name;
use crate::render::paint;

pub(in crate::task) fn render_dispatch(
    outcome: &DispatchedSession,
    session_identity: &str,
    agent: Agent,
    on: bool,
) -> anyhow::Result<String> {
    match outcome.outcome.as_ref() {
        Some(dispatched_session::Outcome::Aborted(_)) => {
            Ok(render_session_aborted(session_identity))
        }
        Some(dispatched_session::Outcome::InlineLaunch(_)) => {
            Ok(format!("# session {session_identity}: ran inline\n"))
        }
        Some(dispatched_session::Outcome::WindowOpened(opened)) => {
            let line = paint(
                &format!("session: {}  ·  window: {}", opened.session, opened.window),
                AnsiColor::Green,
                on,
            );
            let mut out = format!("# session {session_identity}: dispatched\n{line}\n");
            let _ = write!(
                out,
                "agent: {}\n\
outside tmux: tmux attach-session -t ={}\n\
inside tmux: tmux switch-client -t ={}\n",
                agent_name(agent),
                opened.session,
                opened.session,
            );
            Ok(out)
        }
        None => Err(anyhow::anyhow!(
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
    let identity = &launch.session_name;
    let first_task_id = launch
        .task_ids
        .first()
        .ok_or_else(|| anyhow::anyhow!("pwf-server returned a dry run without task IDs"))?;
    let target = match mode {
        DispatchMode::Inline => "inline".to_string(),
        DispatchMode::Multiplexer => {
            format!("tmux: {} / {}", session_name(first_task_id), identity)
        }
        DispatchMode::Unspecified => "unknown target".to_string(),
    };
    let effort = SessionEffort::try_from(launch.effort).unwrap_or(SessionEffort::Unspecified);
    let task_details = if launch.task_ids.len() > 1 {
        format!(
            "tasks: {}\nthread: {}",
            launch.task_ids.join(", "),
            launch.title
        )
    } else {
        format!("task: {}", launch.title)
    };
    Ok(format!(
        "# session {} - dry run\n\
{}\n\
agent: {}\n\
model: {}\n\
effort: {}\n\
project_path: {}\n\
{target}\n\
command: {}\n\
nothing dispatched.\n",
        identity,
        task_details,
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
    use pwf_client::pb::{AbortedSession, AgentLaunch, InlineLaunch, SessionPlan, WindowOpened};

    use super::*;

    #[test]
    fn success_renders_green_token_and_bold_session_window_plain() {
        let outcome = DispatchedSession {
            outcome: Some(dispatched_session::Outcome::WindowOpened(WindowOpened {
                session: "aux".to_string(),
                window: "AUX-0009".to_string(),
            })),
        };
        let out = render_dispatch(&outcome, "AUX-0009", Agent::Claude, false).unwrap();
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
            outcome: Some(dispatched_session::Outcome::InlineLaunch(InlineLaunch {
                argv: vec!["codex".to_string()],
                working_directory: "/project".to_string(),
            })),
        };
        assert_eq!(
            render_dispatch(&outcome, "FOO-0001", Agent::Codex, false).unwrap(),
            "# session FOO-0001: ran inline\n"
        );
        let aborted = DispatchedSession {
            outcome: Some(dispatched_session::Outcome::Aborted(AbortedSession {})),
        };
        assert_eq!(
            render_dispatch(&aborted, "FOO-0001", Agent::Codex, false).unwrap(),
            "# session FOO-0001: aborted\nnothing dispatched.\n"
        );
    }

    #[test]
    fn dry_run_renders_reasoning_effort() {
        let outcome = PlanSessionResponse {
            plan: Some(SessionPlan {
                launch: Some(AgentLaunch {
                    agent: Agent::Codex as i32,
                    task_ids: vec!["FOO-0001".to_string()],
                    title: "FOO-0001 - reason carefully".to_string(),
                    project_path: "/project".to_string(),
                    prompt: "Inspect FOO-0001.".to_string(),
                    model: None,
                    effort: SessionEffort::High as i32,
                    session_name: "FOO-0001".to_string(),
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

    #[test]
    fn multi_task_dispatch_renders_the_compound_identity_and_window() {
        let outcome = DispatchedSession {
            outcome: Some(dispatched_session::Outcome::WindowOpened(WindowOpened {
                session: "foo".to_string(),
                window: "foo15,foo23".to_string(),
            })),
        };

        let out = render_dispatch(&outcome, "foo15,foo23", Agent::Codex, false).unwrap();

        assert!(out.starts_with("# session foo15,foo23: dispatched"));
        assert!(out.contains("window: foo15,foo23"));
    }
}
