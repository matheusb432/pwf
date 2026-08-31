//! Process-neutral session request and response contracts.

use pwf_models::{
    AppDate,
    project::Project,
    revision::ContentRevision,
    session::{
        Agent, AgentModel, DispatchMode, LaunchDirectives, LaunchPrompt, PushedPrompt,
        SessionEffort, SessionTaskIds, SessionThreadTitle, SessionWorkingDirectory,
    },
    task::TaskId,
};

use super::{BlockedByIssue, BlockedByStatus, TaskHeading};

/// Requests one provider-neutral session plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanSession {
    pub task_ids: SessionTaskIds,
    pub intent: PlanSessionIntent,
    pub pushed_prompt: Option<PushedPrompt>,
    pub mode: DispatchMode,
    pub directives: LaunchDirectives,
    pub agent: Agent,
    pub model_override: AgentModel,
    pub effort: SessionEffort,
}

/// Selects whether a plan is prepared for dispatch or rendered without effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanSessionIntent {
    Dispatch,
    DryRun,
}

/// Describes a provider-neutral agent launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentLaunch {
    pub agent: Agent,
    pub task_ids: SessionTaskIds,
    pub title: SessionThreadTitle,
    pub project_path: SessionWorkingDirectory,
    pub prompt: LaunchPrompt,
    pub model: AgentModel,
    pub effort: SessionEffort,
}

/// Groups a provider-neutral launch with its mechanical dispatch destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionPlan {
    pub launch: AgentLaunch,
    pub mode: DispatchMode,
}

impl SessionPlan {
    #[must_use]
    pub fn target(&self) -> DispatchTarget {
        DispatchTarget::new(self.launch.task_ids.clone())
    }
}

/// Describes a validated dry-run plan and its exact process arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DryRunSession {
    pub plan: SessionPlan,
    pub argv: Vec<String>,
    pub probe: AgentProbe,
    pub warnings: Vec<SessionWarning>,
}

/// Contains a validated session dispatch ready for confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedSessionDispatch {
    pub project: Box<Project>,
    pub plan: SessionPlan,
    pub confirmation: DispatchConfirmation,
    pub probe: AgentProbe,
    pub warnings: Vec<SessionWarning>,
    pub task_revisions: Vec<PreparedTaskRevision>,
}

/// Retains one preflight task revision without exposing it in the transport response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedTaskRevision {
    pub task_id: TaskId,
    pub revision: ContentRevision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionWarning {
    BlockedBy(BlockedByStatus),
    BlockedByMetadata(BlockedByIssue),
}

/// Carries either a dispatch-ready plan or a side-effect-free preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlannedSession {
    Dispatch(PreparedSessionDispatch),
    DryRun(DryRunSession),
}

/// Describes the host target reached by a dispatched session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchedSession {
    Aborted {
        task_ids: SessionTaskIds,
    },
    InlineLaunch {
        task_ids: SessionTaskIds,
        argv: Vec<String>,
        working_directory: SessionWorkingDirectory,
    },
    WindowOpened {
        target: DispatchTarget,
        agent: Agent,
        project_path: SessionWorkingDirectory,
    },
}

/// Identifies a multiplexer session and window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchTarget(SessionTaskIds);

impl DispatchTarget {
    #[must_use]
    pub fn new(task_ids: SessionTaskIds) -> Self {
        Self(task_ids)
    }

    #[must_use]
    pub fn task_ids(&self) -> &SessionTaskIds {
        &self.0
    }

    /// Returns the lowercase tmux session name derived from the project ID.
    #[must_use]
    pub fn multiplexer_session_name(&self) -> String {
        self.0.project_id().as_ref().to_ascii_lowercase()
    }

    /// Returns the stable tmux window and agent-session name.
    #[must_use]
    pub fn window_name(&self) -> String {
        self.0.identity()
    }
}

/// Contains the context shown before an interactive dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchConfirmation {
    pub task_ids: SessionTaskIds,
    /// Heading of the first task, retained for singleton compatibility.
    pub title: TaskHeading,
    /// Creation date of the first task, retained for singleton compatibility.
    pub created: Option<AppDate>,
    pub mode: DispatchMode,
    pub agent: Agent,
    pub directives: LaunchDirectives,
    pub has_pushed_prompt: bool,
    pub model: AgentModel,
    pub effort: SessionEffort,
}

impl DispatchConfirmation {
    #[must_use]
    pub fn target(&self) -> DispatchTarget {
        DispatchTarget::new(self.task_ids.clone())
    }
}

/// Reports whether the selected agent's executable is available.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentProbe {
    pub agent: Agent,
    pub availability: AgentAvailability,
}

impl AgentProbe {
    #[must_use]
    pub const fn is_available(&self) -> bool {
        matches!(&self.availability, AgentAvailability::Available)
    }

    #[must_use]
    pub const fn binary(&self) -> &'static str {
        match self.agent {
            Agent::Claude => "claude",
            Agent::Codex => "codex",
        }
    }
}

/// Classifies one agent executable as missing or available.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentAvailability {
    Missing,
    Available,
}

#[cfg(test)]
mod tests {
    use pwf_models::session::SessionTaskIds;

    use super::DispatchTarget;

    #[test]
    fn dispatch_target_separates_the_project_session_from_the_compound_window() {
        let target = DispatchTarget::new(
            SessionTaskIds::try_new(["aux9".parse().unwrap(), "aux2".parse().unwrap()]).unwrap(),
        );

        assert_eq!(target.multiplexer_session_name(), "aux");
        assert_eq!(target.window_name(), "aux2,aux9");
    }
}
