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
use crate::{task::session, v1};

pub fn plan_session_request(
    request: v1::PlanSessionRequest,
) -> Result<session::PlanSession, Status> {
    let intent = match v1::PlanSessionIntent::try_from(request.intent).ok() {
        Some(v1::PlanSessionIntent::DryRun) => session::PlanSessionIntent::DryRun,
        Some(v1::PlanSessionIntent::Dispatch) => session::PlanSessionIntent::Dispatch,
        Some(v1::PlanSessionIntent::Unspecified) | None => {
            return Err(invalid("intent", "must be specified"));
        }
    };
    let mode = match v1::DispatchMode::try_from(request.mode).ok() {
        Some(v1::DispatchMode::Inline) => DispatchMode::Inline,
        Some(v1::DispatchMode::Multiplexer) => DispatchMode::Multiplexer,
        Some(v1::DispatchMode::Unspecified) | None => {
            return Err(invalid("mode", "must be specified"));
        }
    };
    let agent = match v1::Agent::try_from(request.agent).ok() {
        Some(v1::Agent::Claude) => Agent::Claude,
        Some(v1::Agent::Codex) => Agent::Codex,
        Some(v1::Agent::Unspecified) | None => {
            return Err(invalid("agent", "must be specified"));
        }
    };
    let effort = match v1::SessionEffort::try_from(request.effort).ok() {
        Some(v1::SessionEffort::Low) => SessionEffort::Low,
        Some(v1::SessionEffort::Medium) => SessionEffort::Medium,
        Some(v1::SessionEffort::High) => SessionEffort::High,
        Some(v1::SessionEffort::Xhigh) => SessionEffort::XHigh,
        Some(v1::SessionEffort::Max) => SessionEffort::Max,
        Some(v1::SessionEffort::Unspecified) | None => {
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

pub fn plan_session_response(session: session::DryRunSession) -> v1::PlanSessionResponse {
    v1::PlanSessionResponse {
        plan: Some(session_plan(session.plan)),
        argv: session.argv,
        probe: Some(agent_probe(&session.probe)),
        warnings: session.warnings.into_iter().map(session_warning).collect(),
    }
}

pub fn dispatch_session_preflight(
    session: &session::PreparedSessionDispatch,
) -> v1::SessionDispatchPreflight {
    v1::SessionDispatchPreflight {
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
pub fn dispatch_session_result(session: session::DispatchedSession) -> v1::DispatchedSession {
    match session {
        session::DispatchedSession::Aborted { task_ids } => {
            let session_name = task_ids.identity();
            v1::DispatchedSession {
                outcome: v1::DispatchSessionOutcome::Aborted as i32,
                task_ids: task_id_values(&task_ids),
                inline_launch: None,
                window_opened: None,
                session_name,
            }
        }
        session::DispatchedSession::InlineLaunch {
            task_ids,
            argv,
            working_directory,
        } => {
            let task_id_values = task_id_values(&task_ids);
            let session_name = task_ids.identity();
            v1::DispatchedSession {
                outcome: v1::DispatchSessionOutcome::InlineLaunch as i32,
                task_ids: task_id_values.clone(),
                inline_launch: Some(v1::InlineLaunch {
                    task_ids: task_id_values,
                    argv,
                    working_directory: working_directory.to_string(),
                    session_name: session_name.clone(),
                }),
                window_opened: None,
                session_name,
            }
        }
        session::DispatchedSession::WindowOpened {
            target,
            agent,
            project_path,
        } => {
            let task_id_values = task_id_values(target.task_ids());
            let window_name = target.window_name();
            v1::DispatchedSession {
                outcome: v1::DispatchSessionOutcome::WindowOpened as i32,
                task_ids: task_id_values.clone(),
                inline_launch: None,
                window_opened: Some(v1::WindowOpened {
                    task_ids: task_id_values,
                    session: target.multiplexer_session_name(),
                    agent: agent_value(agent),
                    project_path: project_path.to_string(),
                    window: window_name.clone(),
                }),
                session_name: window_name,
            }
        }
    }
}

fn session_plan(plan: session::SessionPlan) -> v1::SessionPlan {
    let session::SessionPlan { launch, mode } = plan;
    let session_name = launch.task_ids.identity();
    v1::SessionPlan {
        launch: Some(v1::AgentLaunch {
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

fn dispatch_confirmation(confirmation: &session::DispatchConfirmation) -> v1::DispatchConfirmation {
    v1::DispatchConfirmation {
        task_ids: task_id_values(&confirmation.task_ids),
        title: confirmation.title.to_string(),
        created: confirmation.created.as_ref().map(ToString::to_string),
        mode: dispatch_mode_value(confirmation.mode),
        agent: agent_value(confirmation.agent),
        directives: Some(v1::LaunchDirectives {
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

fn agent_probe(probe: &session::AgentProbe) -> v1::AgentProbe {
    let availability = match probe.availability {
        session::AgentAvailability::Missing => v1::AgentAvailability::Missing,
        session::AgentAvailability::Available => v1::AgentAvailability::Available,
    };
    v1::AgentProbe {
        agent: agent_value(probe.agent),
        availability: availability as i32,
    }
}

fn session_warning(warning: session::SessionWarning) -> v1::SessionWarning {
    let value = match warning {
        session::SessionWarning::BlockedBy(value) => {
            v1::session_warning::Value::BlockedBy(task::blocked_by_status(value))
        }
        session::SessionWarning::BlockedByMetadata(value) => {
            v1::session_warning::Value::BlockedByMetadata(task::blocked_by_issue(value))
        }
    };
    v1::SessionWarning { value: Some(value) }
}

fn agent_value(agent: Agent) -> i32 {
    match agent {
        Agent::Claude => v1::Agent::Claude as i32,
        Agent::Codex => v1::Agent::Codex as i32,
    }
}

fn dispatch_mode_value(mode: DispatchMode) -> i32 {
    match mode {
        DispatchMode::Inline => v1::DispatchMode::Inline as i32,
        DispatchMode::Multiplexer => v1::DispatchMode::Multiplexer as i32,
    }
}

fn effort_value(effort: SessionEffort) -> i32 {
    match effort {
        SessionEffort::Low => v1::SessionEffort::Low as i32,
        SessionEffort::Medium => v1::SessionEffort::Medium as i32,
        SessionEffort::High => v1::SessionEffort::High as i32,
        SessionEffort::XHigh => v1::SessionEffort::Xhigh as i32,
        SessionEffort::Max => v1::SessionEffort::Max as i32,
    }
}

#[cfg(test)]
mod tests {
    use tonic::Code;

    use super::*;

    fn request(task_ids: &[&str]) -> v1::PlanSessionRequest {
        v1::PlanSessionRequest {
            task_ids: task_ids.iter().map(ToString::to_string).collect(),
            intent: v1::PlanSessionIntent::DryRun as i32,
            pushed_prompt: None,
            mode: v1::DispatchMode::Inline as i32,
            directives: Some(v1::LaunchDirectives::default()),
            agent: v1::Agent::Codex as i32,
            model_override: None,
            effort: v1::SessionEffort::High as i32,
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
