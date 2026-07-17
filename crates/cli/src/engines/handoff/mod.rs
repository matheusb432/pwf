//! Implements handoff scaffolding, listing, ledger reconciliation, and lifecycle mirroring.

mod actions;
mod errors;
mod ledger;
pub(crate) mod mirror;
mod paths;
mod pw_bridge;
mod run;
mod scaffold;
#[cfg(test)]
mod test_support;

pub use paths::slug;
pub use run::run;
pub use scaffold::scaffold;
