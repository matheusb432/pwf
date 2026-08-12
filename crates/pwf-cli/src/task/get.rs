use clap::Args;
use pwf_application::task::get_task::{self, GetTask};
use pwf_infra::obsidian::ObsidianStore;
use pwf_wire::task::{TaskRead, TaskReadFormat};

use super::Identifier;

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

use super::TaskError;

/// Returns the complete Markdown for a task regardless of status;
/// `--path` returns the note path instead.
pub(super) async fn run(
    arguments: &Arguments,
    store: &ObsidianStore,
    pool: &sqlx::SqlitePool,
) -> Result<String, TaskError> {
    let id = arguments.identifier.required("get")?;
    let output = if arguments.path {
        TaskReadFormat::Path
    } else if arguments.json {
        TaskReadFormat::Data
    } else {
        TaskReadFormat::Markdown
    };
    let gotten = get_task::execute(&GetTask { id, output }, store, pool).await?;
    match gotten {
        TaskRead::Markdown(markdown) => Ok(markdown),
        TaskRead::Path(path) => Ok(path.as_path().to_string_lossy().into_owned()),
        TaskRead::Data(task) => Ok(output::json(*task)?),
    }
}
