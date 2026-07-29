//! Model-catalog boundary for session operations.

use pwf_models::pending_work::EffortTier;

use super::ModelTierLookup;

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
