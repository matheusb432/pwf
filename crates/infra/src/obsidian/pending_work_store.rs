mod error;
mod fs;
mod lookup;
mod read;
mod resolve;
mod status;
#[cfg(test)]
mod tests;
mod write;

use pwf_core::config::Config;

pub use error::ObsidianPendingWorkStoreError;

#[derive(Clone)]
pub struct ObsidianPendingWorkStore {
    config: Config,
}

impl ObsidianPendingWorkStore {
    pub fn new(config: Config) -> Self {
        Self { config }
    }
}
