//! Defines application-owned session planning policy and semantic values.

pub mod dispatch_session;
mod host;
mod launch;
mod model;
mod model_selection;
pub mod plan_session;
mod ports;
mod provider;
mod task_content;
pub mod verify_session;

pub use host::{
    InlineSessionClient, RepositorySessionClient, ZellijSessionClient, ZellijTabOpenError,
};
pub use model::{
    Agent, AgentLaunch, AgentProbe, DispatchConfirmation, DispatchMode, DispatchTarget,
    LaunchDirectives, ModelTier, ModelTierLookup, SessionPlan, VerifySessionOk,
};
pub use plan_session::PlanSessionIntent;
pub use ports::ModelTierCatalog;
pub use provider::{ClaudeSessionClient, CodexSessionClient, PreparedCodexLaunch};
