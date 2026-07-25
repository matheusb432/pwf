//! Semantic session values shared by operations and adapters.

use crate::pending_work::PendingWorkItemView;

/// Selects a supported agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agent {
    Claude,
    Codex,
}

/// Selects where an agent runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchMode {
    Inline,
    Multiplexer,
}

/// Selects optional launch-prompt directives.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LaunchDirectives {
    pub worktree: bool,
    pub autonomous: bool,
}

/// Describes a provider-neutral agent launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentLaunch {
    pub agent: Agent,
    pub task_id: String,
    pub title: String,
    pub repository: String,
    pub prompt: String,
    pub model: Option<String>,
}
impl AgentLaunch {
    pub fn new(
        item: &PendingWorkItemView,
        task_content: &str,
        directives: LaunchDirectives,
        agent: Agent,
        model: Option<String>,
    ) -> Self {
        AgentLaunch {
            agent,
            task_id: item.id.clone(),
            title: super::launch::thread_title(item),
            repository: item.repo.clone().unwrap_or_default(),
            prompt: super::launch::launch_prompt(task_content, &item.id, directives),
            model,
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

/// Identifies a multiplexer session and tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchTarget {
    pub session: String,
    pub tab: String,
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

/// Contains pending-work launchability and an optional provider-neutral launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifySessionOk {
    pub task_id: Option<String>,
    pub launchable: bool,
    pub issues: Vec<String>,
    pub launch: Option<AgentLaunch>,
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

/// The agent harness' model.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AgentModel(Option<String>);

impl From<Option<String>> for AgentModel {
    fn from(value: Option<String>) -> Self {
        Self(value.filter(|model| model != Self::MODEL_DEFAULT))
    }
}

impl From<String> for AgentModel {
    fn from(value: String) -> Self {
        Some(value).into()
    }
}

impl From<Option<&str>> for AgentModel {
    fn from(value: Option<&str>) -> Self {
        value.map(str::to_string).into()
    }
}

impl AgentModel {
    pub const MODEL_DEFAULT: &str = "default";

    pub fn into_inner(self) -> Option<String> {
        self.0
    }

    pub fn display_or_default(&self) -> String {
        self.0.clone().unwrap_or(Self::MODEL_DEFAULT.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::AgentModel;

    #[test]
    fn default_and_absent_models_are_no_override() {
        assert_eq!(AgentModel::from(None::<String>).into_inner(), None);
        assert_eq!(
            AgentModel::from(Some("default".to_string())).into_inner(),
            None
        );
        assert_eq!(
            AgentModel::from(Some("gpt-5.6".to_string())).into_inner(),
            Some("gpt-5.6".to_string())
        );
    }
}
