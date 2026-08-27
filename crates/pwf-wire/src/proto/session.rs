//! Explicit protobuf mappings for session operations.

use pwf_models::{
    session::{Agent, AgentModel, DispatchMode, LaunchDirectives, PushedPrompt, SessionEffort},
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
        task_id: parse::<TaskId>("task_id", &request.task_id)?,
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

pub fn dispatch_session_result(session: session::DispatchedSession) -> v1::DispatchedSession {
    match session {
        session::DispatchedSession::Aborted { task_id } => v1::DispatchedSession {
            outcome: v1::DispatchSessionOutcome::Aborted as i32,
            task_id: task_id.to_string(),
            inline_launch: None,
            window_opened: None,
        },
        session::DispatchedSession::InlineLaunch {
            task_id,
            argv,
            working_directory,
        } => v1::DispatchedSession {
            outcome: v1::DispatchSessionOutcome::InlineLaunch as i32,
            task_id: task_id.to_string(),
            inline_launch: Some(v1::InlineLaunch {
                task_id: task_id.to_string(),
                argv,
                working_directory: working_directory.to_string(),
            }),
            window_opened: None,
        },
        session::DispatchedSession::WindowOpened {
            target,
            agent,
            project_path,
        } => v1::DispatchedSession {
            outcome: v1::DispatchSessionOutcome::WindowOpened as i32,
            task_id: target.task_id().to_string(),
            inline_launch: None,
            window_opened: Some(v1::WindowOpened {
                task_id: target.task_id().to_string(),
                session: target.session_name(),
                agent: agent_value(agent),
                project_path: project_path.to_string(),
            }),
        },
    }
}

fn session_plan(plan: session::SessionPlan) -> v1::SessionPlan {
    let session::SessionPlan { launch, mode } = plan;
    v1::SessionPlan {
        launch: Some(v1::AgentLaunch {
            agent: agent_value(launch.agent),
            task_id: launch.task_id.to_string(),
            title: launch.title.to_string(),
            project_path: launch.project_path.to_string(),
            prompt: launch.prompt.to_string(),
            model: launch.model.as_deref().map(str::to_string),
            effort: effort_value(launch.effort),
        }),
        mode: dispatch_mode_value(mode),
    }
}

fn dispatch_confirmation(confirmation: &session::DispatchConfirmation) -> v1::DispatchConfirmation {
    v1::DispatchConfirmation {
        task_id: confirmation.task_id.to_string(),
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
    }
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
