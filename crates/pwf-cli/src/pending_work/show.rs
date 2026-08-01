use clap::Args;
use pwf_application::pending_work::{
    ShowOutput,
    show_pending_work_item::{self, ShowPendingWorkItem, ShowPendingWorkItemOk},
};
use pwf_infra::obsidian::ObsidianStore;

use super::shared::{CommonArguments, Identifier};

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

use super::shared::PendingWorkError;

/// Returns the complete Markdown for an item regardless of status;
/// `--path` returns the note path instead.
pub(super) async fn run(
    arguments: &Arguments,
    store: &ObsidianStore,
    pool: &sqlx::SqlitePool,
) -> Result<String, PendingWorkError> {
    let id = arguments.identifier.required("show")?;
    let output = if arguments.path {
        ShowOutput::Path
    } else if arguments.json {
        ShowOutput::Json
    } else {
        ShowOutput::Markdown
    };
    let shown = show_pending_work_item::execute(&ShowPendingWorkItem { id, output }, store, pool)
        .await
        .map_err(|error| PendingWorkError::ApplicationRead(error.to_string()))?;
    match shown {
        ShowPendingWorkItemOk::Markdown(markdown) => Ok(markdown),
        ShowPendingWorkItemOk::Path(path) => Ok(path),
        ShowPendingWorkItemOk::Json(task) => {
            output::json(*task).map_err(PendingWorkError::ApplicationRead)
        }
    }
}
