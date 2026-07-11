mod add;
mod error;
mod fs;
mod lookup;
mod read;
mod read_parser;
mod remove;
mod resolve;
mod status;
#[cfg(test)]
mod tests;
mod update;
mod write;

pub use error::ObsidianPendingWorkStoreError;
use pwf_core::config::Config;

#[derive(Clone)]
pub struct ObsidianPendingWorkStore {
    config: Config,
}

impl ObsidianPendingWorkStore {
    pub fn new(config: Config) -> Self {
        Self { config }
    }
}
