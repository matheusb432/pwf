use clap::Args;
use pwf_client::{
    task::TaskClient,
    v1::{GetTaskRequest, TaskReadFormat, get_task_response},
};

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

/// Returns the complete Markdown for a task regardless of status;
/// `--path` returns the note path instead.
pub(super) async fn run(arguments: &Arguments, client: &TaskClient) -> anyhow::Result<String> {
    let id = arguments
        .identifier
        .required(anyhow::anyhow!("--id is required for get."))?;
    let output = if arguments.path {
        TaskReadFormat::Path
    } else if arguments.json {
        TaskReadFormat::Data
    } else {
        TaskReadFormat::Markdown
    };
    let gotten = client
        .get_task(GetTaskRequest {
            id: id.to_string(),
            output: output as i32,
        })
        .await
        .map_err(crate::rpc_error)?;
    match gotten.value {
        Some(get_task_response::Value::Markdown(markdown)) => Ok(markdown),
        Some(get_task_response::Value::Path(path)) => Ok(path),
        Some(get_task_response::Value::Data(task)) => output::json(*task).map_err(Into::into),
        None => Err(anyhow::anyhow!("pwf-server returned an empty task read")),
    }
}
