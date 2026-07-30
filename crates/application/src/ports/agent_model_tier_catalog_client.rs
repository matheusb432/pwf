use pwf_models::pending_work::EffortTier;

use crate::pending_work::session::ModelTierLookup;

pub trait AgentModelTierCatalogClient: Clone + Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;

    fn tier(&self, effort: EffortTier) -> Result<ModelTierLookup, Self::Error>;
}
