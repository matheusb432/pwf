//! Session operation and capability data transfer objects.

use pwf_models::session::{Agent, DispatchMode, LaunchDirectives, SessionEffort};

use crate::pending_work::PendingWorkItemView;

/// Describes a provider-neutral agent launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentLaunch {
    pub agent: Agent,
    pub task_id: String,
    pub title: String,
    pub repository: String,
    pub prompt: String,
    pub model: Option<String>,
    pub effort: SessionEffort,
}
impl AgentLaunch {
    pub fn new(
        item: &PendingWorkItemView,
        task_content: &str,
        directives: LaunchDirectives,
        agent: Agent,
        model: Option<String>,
        effort: SessionEffort,
    ) -> Self {
        AgentLaunch {
            agent,
            task_id: item.id.clone(),
            title: super::launch::thread_title(item),
            repository: item.repo.clone().unwrap_or_default(),
            prompt: super::launch::launch_prompt(task_content, &item.id, directives),
            model,
            effort,
        }
    }
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
    pub session: String,
    pub window: String,
}

/// Contains the context shown before an interactive dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchConfirmation {
    pub task_id: String,
    pub title: String,
    pub created: Option<String>,
    pub mode: DispatchMode,
    pub agent: Agent,
    pub directives: LaunchDirectives,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifySessionOk {
    pub task_id: Option<String>,
    pub probe: AgentProbe,
    pub launchable: bool,
    pub issues: Vec<String>,
    pub command_argv: Vec<String>,
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
