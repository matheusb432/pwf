use pwf_client::{
    pb::{Agent, DispatchedSession, PlanSessionResponse, SessionEffort, dispatched_session},
    render_argv,
};

pub(in crate::task) fn render_dispatch(
    outcome: &DispatchedSession,
    session_identity: &str,
) -> anyhow::Result<String> {
    match outcome.outcome.as_ref() {
        Some(dispatched_session::Outcome::Aborted(_)) => {
            Ok(render_session_aborted(session_identity))
        }
        Some(dispatched_session::Outcome::InlineLaunch(_)) => {
            Ok(format!("# session {session_identity}: ran inline\n"))
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
    let identity = &launch.session_name;
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
    use pwf_client::pb::{AbortedSession, AgentLaunch, InlineLaunch, SessionPlan};

    use super::*;

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
            render_dispatch(&outcome, "FOO-0001").unwrap(),
            "# session FOO-0001: ran inline\n"
        );
        let aborted = DispatchedSession {
            outcome: Some(dispatched_session::Outcome::Aborted(AbortedSession {})),
        };
        assert_eq!(
            render_dispatch(&aborted, "FOO-0001").unwrap(),
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
