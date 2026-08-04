//! Session operation and capability data transfer objects.

use pwf_models::{
    session::{Agent, DispatchMode, LaunchDirectives, SessionEffort},
    task::TaskId,
};

/// Describes a provider-neutral agent launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentLaunch {
    pub agent: Agent,
    pub task_id: TaskId,
    pub title: String,
    pub project_path: String,
    pub prompt: String,
    pub model: Option<String>,
    pub effort: SessionEffort,
}
/// Groups a provider-neutral launch with its mechanical dispatch destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionPlan {
    pub launch: AgentLaunch,
    pub mode: DispatchMode,
    pub target: DispatchTarget,
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
    pub title: String,
    pub created: Option<String>,
    pub mode: DispatchMode,
    pub agent: Agent,
    pub directives: LaunchDirectives,
    pub has_pushed_prompt: bool,
    pub model: String,
    pub effort: SessionEffort,
    pub target: DispatchTarget,
}

/// Contains an agent binary's availability and discovered metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentProbe {
    pub binary: String,
    pub available: bool,
    pub path: Option<String>,
    pub version: Option<String>,
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
    pub catalog: String,
    pub tier: Option<ModelTier>,
}
