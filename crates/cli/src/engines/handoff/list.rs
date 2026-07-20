//! Translates `handoff list` into the application ledger read.

use std::path::Path;

use clap::Args;
use pwf_application::{
    AppDbStore, HandoffLedger,
    handoff::list::{ListHandoffs, ListedHandoffs},
};
use pwf_infra::obsidian::ObsidianStore;

use super::common::{CommonArguments, HandoffError, repository_root};

#[derive(Args, Debug)]
pub struct Arguments {
    #[command(flatten)]
    pub(crate) common: CommonArguments,
}

pub(super) fn run(arguments: &Arguments) -> Result<String, HandoffError> {
    let root = repository_root(&arguments.common)?;
    let configuration = crate::config::from_json("{}", None)
        .expect("an empty configuration is valid for repository-scoped handoff records");
    invoke_list(&root, &ObsidianStore::new(configuration))
}

pub(in crate::engines::handoff) fn invoke_list<S>(
    root: &Path,
    store: &S,
) -> Result<String, HandoffError>
where
    S: AppDbStore<HandoffLedger>,
{
    let listed = pwf_application::handoff::list::execute(
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
