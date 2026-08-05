use clap::Args;
use pwf_application::task::{
    ShowOutput,
    show_task::{self, ShowTask, ShowTaskOk},
};
use pwf_infra::obsidian::ObsidianStore;

use super::shared::Identifier;

mod output;

#[derive(Args, Debug)]
pub struct Arguments {
    #[command(flatten)]
    pub(crate) identifier: Identifier,
    /// Print the task's note path instead of the note markdown.
    #[arg(long)]
    pub(crate) path: bool,
    /// Print typed task data as JSON.
    #[arg(long, conflicts_with = "path")]
    pub(crate) json: bool,
}

use super::shared::TaskError;

/// Returns the complete Markdown for a task regardless of status;
/// `--path` returns the note path instead.
pub(super) async fn run(
    arguments: &Arguments,
    store: &ObsidianStore,
    pool: &sqlx::SqlitePool,
) -> Result<String, TaskError> {
    let id = arguments.identifier.required("show")?;
    let output = if arguments.path {
        ShowOutput::Path
    } else if arguments.json {
        ShowOutput::Json
    } else {
        ShowOutput::Markdown
    };
    let shown = show_task::execute(&ShowTask { id, output }, store, pool)
        .await
        .map_err(|error| TaskError::ApplicationRead(error.to_string()))?;
    match shown {
        ShowTaskOk::Markdown(markdown) => Ok(markdown),
        ShowTaskOk::Path(path) => Ok(path),
        ShowTaskOk::Json(task) => output::json(*task).map_err(TaskError::ApplicationRead),
    }
}
