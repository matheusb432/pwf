//! Process-neutral session request and response contracts.

use std::path::PathBuf;

use pwf_models::{
    AppDate,
    session::{
        Agent, AgentModel, DispatchMode, LaunchDirectives, LaunchPrompt, SessionEffort,
        SessionThreadTitle, SessionWorkingDirectory,
    },
    task::TaskId,
};

use super::TaskHeading;

/// Describes a provider-neutral agent launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentLaunch {
    pub agent: Agent,
    pub task_id: TaskId,
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
    pub target: DispatchTarget,
}

/// Describes a validated dry-run plan and its exact process arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DryRunSession {
    pub plan: SessionPlan,
    pub argv: Vec<String>,
    pub probe: AgentProbe,
}

/// Contains a validated session dispatch ready for confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedSessionDispatch {
    pub plan: SessionPlan,
    pub confirmation: DispatchConfirmation,
    pub probe: AgentProbe,
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
    Inline {
        task_id: TaskId,
    },
    WindowOpened {
        target: DispatchTarget,
        agent: Agent,
        project_path: SessionWorkingDirectory,
    },
}

/// Identifies a multiplexer session and window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchTarget {
    pub task_id: TaskId,
}

impl DispatchTarget {
    /// Returns the lowercase tmux session name derived from the project ID.
    #[must_use]
    pub fn session_name(&self) -> String {
        self.task_id.project_id().as_ref().to_ascii_lowercase()
    }
}

/// Contains the context shown before an interactive dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchConfirmation {
    pub task_id: TaskId,
    pub title: TaskHeading,
    pub created: Option<AppDate>,
    pub mode: DispatchMode,
    pub agent: Agent,
    pub directives: LaunchDirectives,
    pub has_pushed_prompt: bool,
    pub model: AgentModel,
    pub effort: SessionEffort,
    pub target: DispatchTarget,
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

/// Contains a raw model-tier catalog entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelTier {
    /// Preserves an empty configured value for application validation.
    pub claude_model: Option<String>,
}

/// Contains a model-tier result and its diagnostic-facing catalog path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelTierLookup {
    pub catalog: PathBuf,
    pub tier: Option<ModelTier>,
}

#[cfg(test)]
mod tests {
    use pwf_models::task::TaskId;

    use super::DispatchTarget;

    #[test]
    fn session_name_lowercases_the_typed_project_id() {
        let target = DispatchTarget {
            task_id: "cfg9".parse::<TaskId>().unwrap(),
        };

        assert_eq!(target.session_name(), "cfg");
    }
}
