//! Semantic session values shared by operations and adapters.

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

/// Selects whether dispatch requires operator confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmationPolicy {
    Skip,
    /// Confirms before append persistence or any launch side effect.
    Ask,
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

/// Contains an agent binary's availability and discovered metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentProbe {
    pub binary: String,
    pub available: bool,
    pub path: Option<String>,
    pub version: Option<String>,
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
    pub target: DispatchTarget,
}

/// Describes a dispatch attempt without pre-rendering CLI output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchSessionOutcome {
    Aborted {
        task_id: String,
    },
    Inline {
        task_id: String,
    },
    /// Records a tab launch into an existing session.
    Direct {
        target: DispatchTarget,
        agent: Agent,
        repository: String,
    },
    /// Records a tab launch after recreating its missing session.
    Recovered {
        target: DispatchTarget,
        agent: Agent,
        repository: String,
    },
    /// Carries a tab-open failure as an outcome rather than an operation error.
    Failed {
        target: DispatchTarget,
        message: String,
    },
}

/// Contains agent availability and optional item launchability results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifySessionOutcome {
    pub task_id: Option<String>,
    pub probe: AgentProbe,
    /// Reports item and model readiness independently of agent availability.
    pub launchable: bool,
    pub issues: Vec<String>,
    pub command_preview: String,
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

/// Classifies a provider-specific tab-open failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TabOpenError {
    SessionNotFound,
    Other(String),
}

#[cfg(test)]
mod tests {
    use super::super::{Agent, DispatchSessionOutcome, DispatchTarget};

    fn target() -> DispatchTarget {
        DispatchTarget {
            session: "pwf".to_string(),
            tab: "PWF-0139".to_string(),
        }
    }

    #[test]
    fn direct_dispatch_is_a_distinct_outcome() {
        let outcome = DispatchSessionOutcome::Direct {
            target: target(),
            agent: Agent::Claude,
            repository: "/repo/pwf".to_string(),
        };

        assert!(matches!(outcome, DispatchSessionOutcome::Direct { .. }));
    }

    #[test]
    fn recovered_dispatch_is_a_distinct_outcome() {
        let outcome = DispatchSessionOutcome::Recovered {
            target: target(),
            agent: Agent::Claude,
            repository: "/repo/pwf".to_string(),
        };

        assert!(matches!(outcome, DispatchSessionOutcome::Recovered { .. }));
    }

    #[test]
    fn failed_dispatch_retains_the_provider_message() {
        let outcome = DispatchSessionOutcome::Failed {
            target: target(),
            message: "session unavailable".to_string(),
        };

        assert!(matches!(
            outcome,
            DispatchSessionOutcome::Failed { ref message, .. }
                if message == "session unavailable"
        ));
    }
}
