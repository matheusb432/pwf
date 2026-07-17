//! Runtime, model-catalog, and interaction boundaries for session operations.

use pwf_domain::pending_work::EffortTier;

use super::{
    Agent, AgentLaunch, AgentProbe, DispatchConfirmation, DispatchTarget, ModelTierLookup,
    TabOpenError,
};

/// Provides host runtime capabilities to session operations.
pub trait SessionRuntime: Clone + Send + Sync + 'static {
    fn probe_agent(&self, agent: Agent) -> AgentProbe;

    fn repository_is_directory(&self, path: &str) -> bool;

    fn multiplexer_available(&self) -> bool;

    fn command_preview(&self, launch: &AgentLaunch) -> String;

    /// Runs the prepared launch in the current terminal.
    ///
    /// # Errors
    ///
    /// Returns the provider message when the launch cannot run or exits unsuccessfully.
    fn run_inline(&self, launch: &AgentLaunch) -> Result<(), String>;

    /// Opens the prepared launch in the target multiplexer tab.
    ///
    /// # Errors
    ///
    /// Returns [`TabOpenError`] when the session is absent or the provider rejects the tab.
    fn open_tab(&self, target: &DispatchTarget, launch: &AgentLaunch) -> Result<(), TabOpenError>;

    /// Creates or resurrects the named multiplexer session.
    ///
    /// # Errors
    ///
    /// Returns the provider message when the session cannot be created or restored.
    fn ensure_session(&self, session: &str) -> Result<(), String>;
}

/// Loads model-tier entries without owning selection policy.
pub trait ModelTierCatalog: Clone + Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;

    /// Loads the entry for an effort tier.
    ///
    /// # Errors
    ///
    /// Returns the adapter error when the catalog cannot be read or parsed.
    fn tier(&self, effort: EffortTier) -> Result<ModelTierLookup, Self::Error>;
}

/// Provides user interaction effects to session operations.
pub trait SessionInteraction: Clone + Send + Sync + 'static {
    /// Warns without blocking dispatch.
    fn warn_agent_missing(&self, binary: &str);

    /// Requests confirmation before any launch side effect.
    fn confirm(&self, context: &DispatchConfirmation) -> bool;

    /// Reports inline dispatch after any required confirmation succeeds.
    fn inline_starting(&self, task_id: &str, repository: &str);
}
