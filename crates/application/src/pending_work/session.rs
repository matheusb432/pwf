//! Defines application-owned session planning policy.

use pwf_models::session::{Agent, DispatchMode, LaunchDirectives, SessionEffort};

pub mod dispatch_session;
mod dto;
mod host;
mod launch;
mod model_selection;
pub mod plan_session;
mod ports;
mod provider;
mod task_content;
pub mod verify_session;

pub use dto::{
    AgentLaunch, AgentProbe, DispatchConfirmation, DispatchTarget, ModelTier, ModelTierLookup,
    SessionPlan, VerifySessionOk,
};
pub use host::{InlineSessionClient, RepositorySessionClient, TmuxSessionClient};
pub use plan_session::PlanSessionIntent;
pub use ports::ModelTierCatalog;
pub use provider::{ClaudeSessionClient, CodexSessionClient, PreparedCodexLaunch};
