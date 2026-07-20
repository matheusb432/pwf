mod add;
mod error;
mod fs;
mod handoff_document;
mod handoff_ledger;
mod index_entry;
mod item_record;
mod lookup;
mod read;
mod read_parser;
#[cfg(test)]
mod tests;

pub use error::ObsidianStoreError;
use pwf_core::config::Config;

#[derive(Clone)]
pub struct ObsidianStore {
    config: Config,
}

impl ObsidianStore {
    pub fn new(config: Config) -> Self {
        Self { config }
    }
}
