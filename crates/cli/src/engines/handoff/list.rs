//! Translates `handoff list` into the application ledger read.

use std::path::Path;

use clap::Args;
use pwf_application::{
    AppRecordStore, HandoffLedger,
    handoff::list::{self, ListHandoffs, ListedHandoffs},
};

use super::common::{CommonArguments, HandoffError, repository_root};

#[derive(Args, Debug)]
pub struct Arguments {
    #[command(flatten)]
    pub(crate) common: CommonArguments,
}

pub(super) fn run(
    arguments: &Arguments,
    store: &impl AppRecordStore<HandoffLedger>,
) -> Result<String, HandoffError> {
    let root = repository_root(&arguments.common)?;
    invoke_list(&root, store)
}

pub(in crate::engines::handoff) fn invoke_list<S>(
    root: &Path,
    store: &S,
) -> Result<String, HandoffError>
where
    S: AppRecordStore<HandoffLedger>,
{
    let listed = list::execute(
        ListHandoffs {
            scope: pwf_application::HandoffScope {
                repository_root: root.to_path_buf(),
            },
        },
        store,
    )
    .map_err(|source| HandoffError::List { source })?;
    Ok(match listed {
        ListedHandoffs::Ledger { markdown } => markdown,
        ListedHandoffs::LedgerMissing => "No active handoffs (LEDGER.md not found).".to_string(),
    })
}
