use clap::Args;
use pwf_client::{pb::CompleteTaskRequest, task::TaskClient};
use pwf_models::{settings::TaskStatusColors, task::TaskReport};

use super::Identifier;
use crate::console::Console;

#[derive(Args, Debug)]
pub struct Arguments {
    #[command(flatten)]
    pub(crate) identifier: Identifier,
    /// Append a one-line completion report.
    #[arg(long)]
    pub(crate) report: Option<String>,
    /// Commit range(s) to record as provenance (repeat or comma-separate).
    #[arg(long)]
    pub(crate) commits: Vec<String>,
}

use super::render::{TaskMutationAction, render_mutation};

pub(super) async fn run(
    arguments: &Arguments,
    console: Console,
    task_status_colors: TaskStatusColors,
    client: &TaskClient,
) -> anyhow::Result<String> {
    let id = arguments
        .identifier
        .required(anyhow::anyhow!("--id is required for done."))?;
    let result = client
        .complete_task(CompleteTaskRequest {
            id: id.to_string(),
            report: arguments
                .report
                .as_deref()
                .map(str::parse::<TaskReport>)
                .transpose()
                .map_err(|error| anyhow::anyhow!(error.to_string()))?
                .map(|report| report.to_string()),
            commits: arguments.commits.clone(),
            expected_revision: None,
            request_id: String::new(),
        })
        .await
        .map_err(crate::rpc_error)?;
    render_mutation(
        TaskMutationAction::Done,
        id.as_ref(),
        result.task.as_ref(),
        task_status_colors,
        console.color(),
    )
}
