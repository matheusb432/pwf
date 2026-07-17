//! Defines application-owned session policy, semantic values, and runtime ports.

pub mod dispatch;
mod launch;
mod model;
mod model_selection;
mod ports;
pub mod verify;

pub use model::{
    Agent, AgentLaunch, AgentProbe, ConfirmationPolicy, DispatchConfirmation, DispatchMode,
    DispatchSessionOutcome, DispatchTarget, LaunchDirectives, ModelTier, ModelTierLookup,
    TabOpenError, VerifySessionOutcome,
};
pub use ports::{ModelTierCatalog, SessionInteraction, SessionRuntime};
