use clap::Args;
use pwf_application::{
    AppRecordStore, NoteMarkdownSource, PendingWorkItem,
    pending_work::{
        ProjectRegistry, ShowOutput,
        show_pending_work_item::{self, ShowPendingWorkItem, ShowPendingWorkItemOutput},
    },
};
use pwf_infra::obsidian::ObsidianStore;

use super::common::{CommonArguments, Identifier};

mod output;

#[derive(Args, Debug)]
pub struct Arguments {
    #[command(flatten)]
    pub(crate) identifier: Identifier,
    /// Print the item's note path instead of the note markdown.
    #[arg(long)]
    pub(crate) path: bool,
    /// Print typed task data as JSON.
    #[arg(long, conflicts_with = "path")]
    pub(crate) json: bool,
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
    } else if args.json {
        ShowOutput::Json
    } else {
        ShowOutput::Markdown
    };
    let shown = show_pending_work_item::execute(
        &ShowPendingWorkItem { id, output },
        store,
        projects,
        store,
    )
    .map_err(|error| PendingWorkError::ApplicationRead(error.to_string()))?;
    match shown {
        ShowPendingWorkItemOutput::Markdown(markdown) => Ok(markdown),
        ShowPendingWorkItemOutput::Path(path) => Ok(path),
        ShowPendingWorkItemOutput::Json(task) => {
            output::json(*task).map_err(PendingWorkError::ApplicationRead)
        }
    }
}
