//! Defines application-owned session planning policy and semantic values.

pub mod dispatch;
mod host;
mod launch;
mod model;
mod model_selection;
pub mod plan;
mod ports;
mod provider;
pub mod verify;

pub use host::{
    InlineSessionClient, RepositorySessionClient, ZellijSessionClient, ZellijTabOpenError,
};
pub use model::{
    Agent, AgentLaunch, AgentProbe, DispatchConfirmation, DispatchMode, DispatchTarget,
    LaunchDirectives, ModelTier, ModelTierLookup, SessionPlan, VerifySessionOk,
};
pub use plan::PlanSessionIntent;
pub use ports::ModelTierCatalog;
pub use provider::{ClaudeSessionClient, CodexSessionClient, PreparedCodexLaunch};
