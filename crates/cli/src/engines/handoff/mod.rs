//! Handoff engine, split by responsibility. `mod.rs` only wires the submodules
//! together and re-exports the public surface; behavior lives in the focused
//! submodules below.

mod actions;
mod errors;
mod ledger;
mod paths;
mod pw_bridge;
mod run;
mod scaffold;
#[cfg(test)]
mod test_support;

pub use paths::slug;
pub use run::run;
pub use scaffold::scaffold;
