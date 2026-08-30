//! Explicit protobuf mappings for session operations.

use pwf_models::{
    session::{
        Agent, AgentModel, DispatchMode, LaunchDirectives, PushedPrompt, SessionEffort,
        SessionTaskIds,
    },
    task::TaskId,
};
use tonic::Status;

use super::{invalid, parse, task};
use crate::{pb, task::session};

pub fn plan_session_request(
    request: pb::PlanSessionRequest,
) -> Result<session::PlanSession, Status> {
    session_request(request.into(), session::PlanSessionIntent::DryRun)
}

pub fn dispatch_session_request(
    request: pb::DispatchSessionStart,
) -> Result<session::PlanSession, Status> {
    session_request(request.into(), session::PlanSessionIntent::Dispatch)
}

struct SessionRequest {
    task_ids: Vec<String>,
    pushed_prompt: Option<String>,
    mode: i32,
    directives: Option<pb::LaunchDirectives>,
    agent: i32,
    model_override: Option<String>,
    effort: i32,
}

impl From<pb::PlanSessionRequest> for SessionRequest {
    fn from(request: pb::PlanSessionRequest) -> Self {
        Self {
            task_ids: request.task_ids,
            pushed_prompt: request.pushed_prompt,
            mode: request.mode,
            directives: request.directives,
            agent: request.agent,
            model_override: request.model_override,
            effort: request.effort,
        }
    }
}

impl From<pb::DispatchSessionStart> for SessionRequest {
    fn from(request: pb::DispatchSessionStart) -> Self {
        Self {
            task_ids: request.task_ids,
            pushed_prompt: request.pushed_prompt,
            mode: request.mode,
            directives: request.directives,
            agent: request.agent,
            model_override: request.model_override,
            effort: request.effort,
        }
    }
}

fn session_request(
    request: SessionRequest,
    intent: session::PlanSessionIntent,
) -> Result<session::PlanSession, Status> {
    let mode = match pb::DispatchMode::try_from(request.mode).ok() {
        Some(pb::DispatchMode::Inline) => DispatchMode::Inline,
        Some(pb::DispatchMode::Multiplexer) => DispatchMode::Multiplexer,
        Some(pb::DispatchMode::Unspecified) | None => {
            return Err(invalid("mode", "must be specified"));
        }
    };
    let agent = match pb::Agent::try_from(request.agent).ok() {
        Some(pb::Agent::Claude) => Agent::Claude,
        Some(pb::Agent::Codex) => Agent::Codex,
        Some(pb::Agent::Unspecified) | None => {
            return Err(invalid("agent", "must be specified"));
        }
    };
    let effort = match pb::SessionEffort::try_from(request.effort).ok() {
        Some(pb::SessionEffort::Low) => SessionEffort::Low,
        Some(pb::SessionEffort::Medium) => SessionEffort::Medium,
        Some(pb::SessionEffort::High) => SessionEffort::High,
        Some(pb::SessionEffort::Xhigh) => SessionEffort::XHigh,
        Some(pb::SessionEffort::Max) => SessionEffort::Max,
        Some(pb::SessionEffort::Unspecified) | None => {
            return Err(invalid("effort", "must be specified"));
        }
    };
    let directives = request.directives.unwrap_or_default();
    Ok(session::PlanSession {
        task_ids: request_task_ids(&request.task_ids)?,
        intent,
        pushed_prompt: request
            .pushed_prompt
            .map(PushedPrompt::try_new)
            .transpose()
            .map_err(|error| invalid("pushed_prompt", error))?,
        mode,
        directives: LaunchDirectives {
            worktree: directives.worktree,
            autonomous: directives.autonomous,
        },
        agent,
        model_override: AgentModel::from(request.model_override),
        effort,
    })
}

pub fn plan_session_response(session: session::DryRunSession) -> pb::PlanSessionResponse {
    pb::PlanSessionResponse {
        plan: Some(session_plan(session.plan)),
        argv: session.argv,
        probe: Some(agent_probe(&session.probe)),
        warnings: session.warnings.into_iter().map(session_warning).collect(),
    }
}

pub fn dispatch_session_preflight(
    session: &session::PreparedSessionDispatch,
) -> pb::SessionDispatchPreflight {
    pb::SessionDispatchPreflight {
        confirmation: Some(dispatch_confirmation(&session.confirmation)),
        probe: Some(agent_probe(&session.probe)),
        warnings: session
            .warnings
            .iter()
            .cloned()
            .map(session_warning)
            .collect(),
    }
}

#[must_use]
pub fn dispatch_session_result(session: session::DispatchedSession) -> pb::DispatchedSession {
    let outcome = match session {
        session::DispatchedSession::Aborted { .. } => {
            pb::dispatched_session::Outcome::Aborted(pb::AbortedSession {})
        }
        session::DispatchedSession::InlineLaunch {
            argv,
            working_directory,
            ..
        } => pb::dispatched_session::Outcome::InlineLaunch(pb::InlineLaunch {
            argv,
            working_directory: working_directory.to_string(),
        }),
        session::DispatchedSession::WindowOpened { target, .. } => {
            pb::dispatched_session::Outcome::WindowOpened(pb::WindowOpened {
                session: target.multiplexer_session_name(),
                window: target.window_name(),
            })
        }
    };
    pb::DispatchedSession {
        outcome: Some(outcome),
    }
}

fn session_plan(plan: session::SessionPlan) -> pb::SessionPlan {
    let session::SessionPlan { launch, mode } = plan;
    let session_name = launch.task_ids.identity();
    pb::SessionPlan {
        launch: Some(pb::AgentLaunch {
            agent: agent_value(launch.agent),
            task_ids: task_id_values(&launch.task_ids),
            title: launch.title.to_string(),
            project_path: launch.project_path.to_string(),
            prompt: launch.prompt.to_string(),
            model: launch.model.as_deref().map(str::to_string),
            effort: effort_value(launch.effort),
            session_name,
        }),
        mode: dispatch_mode_value(mode),
    }
}

fn dispatch_confirmation(confirmation: &session::DispatchConfirmation) -> pb::DispatchConfirmation {
    pb::DispatchConfirmation {
        task_ids: task_id_values(&confirmation.task_ids),
        title: confirmation.title.to_string(),
        created: confirmation.created.as_ref().map(ToString::to_string),
        mode: dispatch_mode_value(confirmation.mode),
        agent: agent_value(confirmation.agent),
        directives: Some(pb::LaunchDirectives {
            worktree: confirmation.directives.worktree,
            autonomous: confirmation.directives.autonomous,
        }),
        has_pushed_prompt: confirmation.has_pushed_prompt,
        model: confirmation.model.as_deref().map(str::to_string),
        effort: effort_value(confirmation.effort),
        session_name: confirmation.task_ids.identity(),
    }
}

fn request_task_ids(task_ids: &[String]) -> Result<SessionTaskIds, Status> {
    let parsed = task_ids
        .iter()
        .map(|task_id| parse::<TaskId>("task_ids", task_id))
        .collect::<Result<Vec<_>, _>>()?;
    SessionTaskIds::try_new(parsed).map_err(|error| invalid("task_ids", error))
}

fn task_id_values(task_ids: &SessionTaskIds) -> Vec<String> {
    task_ids.iter().map(ToString::to_string).collect()
}

fn agent_probe(probe: &session::AgentProbe) -> pb::AgentProbe {
    let availability = match probe.availability {
        session::AgentAvailability::Missing => pb::AgentAvailability::Missing,
        session::AgentAvailability::Available => pb::AgentAvailability::Available,
    };
    pb::AgentProbe {
        agent: agent_value(probe.agent),
        availability: availability as i32,
    }
}

fn session_warning(warning: session::SessionWarning) -> pb::SessionWarning {
    let value = match warning {
        session::SessionWarning::BlockedBy(value) => {
            pb::session_warning::Value::BlockedBy(task::blocked_by_status(value))
        }
        session::SessionWarning::BlockedByMetadata(value) => {
            pb::session_warning::Value::BlockedByMetadata(task::blocked_by_issue(value))
        }
    };
    pb::SessionWarning { value: Some(value) }
}

fn agent_value(agent: Agent) -> i32 {
    match agent {
        Agent::Claude => pb::Agent::Claude as i32,
        Agent::Codex => pb::Agent::Codex as i32,
    }
}

fn dispatch_mode_value(mode: DispatchMode) -> i32 {
    match mode {
        DispatchMode::Inline => pb::DispatchMode::Inline as i32,
        DispatchMode::Multiplexer => pb::DispatchMode::Multiplexer as i32,
    }
}

fn effort_value(effort: SessionEffort) -> i32 {
    match effort {
        SessionEffort::Low => pb::SessionEffort::Low as i32,
        SessionEffort::Medium => pb::SessionEffort::Medium as i32,
        SessionEffort::High => pb::SessionEffort::High as i32,
        SessionEffort::XHigh => pb::SessionEffort::Xhigh as i32,
        SessionEffort::Max => pb::SessionEffort::Max as i32,
    }
}

#[cfg(test)]
mod tests {
    use tonic::Code;

    use super::*;

    fn request(task_ids: &[&str]) -> pb::PlanSessionRequest {
        pb::PlanSessionRequest {
            task_ids: task_ids.iter().map(ToString::to_string).collect(),
            pushed_prompt: None,
            mode: pb::DispatchMode::Inline as i32,
            directives: Some(pb::LaunchDirectives::default()),
            agent: pb::Agent::Codex as i32,
            model_override: None,
            effort: pb::SessionEffort::High as i32,
            environment: std::collections::HashMap::default(),
        }
    }

    #[test]
    fn request_mapping_preserves_multi_task_input_order() {
        let mapped = plan_session_request(request(&["foo23", "foo15"])).unwrap();

        assert_eq!(
            mapped
                .task_ids
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            ["FOO-0023", "FOO-0015"]
        );
        assert_eq!(mapped.task_ids.identity(), "foo15,foo23");
    }

    #[test]
    fn request_mapping_rejects_invalid_collections() {
        let cases = [
            request(&[]),
            request(&["foo1", "foo1"]),
            request(&["foo1", "bar2"]),
            request(&["foo1", "foo2", "foo3", "foo4", "foo5", "foo6"]),
        ];

        for request in cases {
            let error = plan_session_request(request).unwrap_err();
            assert_eq!(error.code(), Code::InvalidArgument);
        }
    }
}
