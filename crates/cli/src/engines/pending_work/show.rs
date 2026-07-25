use clap::Args;
use pwf_application::{
    AppRecordStore, NoteMarkdownSource, PendingWorkItem,
    pending_work::{
        ProjectRegistry, ShowOutput,
        show::{self, ShowPendingWorkItem},
    },
};
use pwf_infra::obsidian::ObsidianStore;

use super::common::{CommonArguments, Identifier};

#[derive(Args, Debug)]
pub struct Arguments {
    #[command(flatten)]
    pub(crate) identifier: Identifier,
    /// Print the item's note path instead of the note markdown.
    #[arg(long)]
    pub(crate) path: bool,
    #[command(flatten)]
    pub(crate) common: CommonArguments,
}

use super::common::PendingWorkError;

pub(super) fn run(
    arguments: &Arguments,
    store: &ObsidianStore,
    projects: &ProjectRegistry,
) -> Result<String, PendingWorkError> {
    run_show(store, projects, arguments)
}

fn lookup_id(arguments: &Arguments) -> Result<String, PendingWorkError> {
    arguments.identifier.required("show")
}

/// Returns the complete Markdown for an item regardless of status;
/// `--path` returns the note path instead.
pub(in crate::engines::pending_work) fn run_show<S>(
    store: &S,
    projects: &ProjectRegistry,
    args: &Arguments,
) -> Result<String, PendingWorkError>
where
    S: AppRecordStore<PendingWorkItem> + NoteMarkdownSource,
{
    let id = lookup_id(args)?;
    let output = if args.path {
        ShowOutput::Path
    } else {
        ShowOutput::Markdown
    };
    show::execute(&ShowPendingWorkItem { id, output }, store, projects, store)
        .map_err(|error| PendingWorkError::ApplicationRead(error.to_string()))
}
