use clap::Args;
use pwf_client::{pb::CancelTaskRequest, task::TaskClient};
use pwf_models::{settings::TaskStatusColors, task::TaskReport};

use super::{
    Identifier,
    render::{TaskMutationAction, render_mutation},
};
use crate::console::Console;

#[derive(Args, Debug)]
pub struct Arguments {
    #[command(flatten)]
    pub(crate) identifier: Identifier,
    /// Required cancellation report: what was tried and why work stopped.
    #[arg(long)]
    pub(crate) report: Option<String>,
    /// Commit range(s) to record as provenance (repeat or comma-separate).
    #[arg(long)]
    pub(crate) commits: Vec<String>,
}

pub(super) async fn run(
    arguments: &Arguments,
    console: Console,
    task_status_colors: TaskStatusColors,
    client: &TaskClient,
) -> anyhow::Result<String> {
    let id = arguments.identifier.id();
    let report = arguments
        .report
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("--report is required for cancel."))?
        .parse::<TaskReport>()
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let command = CancelTaskRequest {
        id: id.to_string(),
        report: report.to_string(),
        commits: arguments.commits.clone(),
        expected_revision: None,
        request_id: String::new(),
    };
    let result = client
        .cancel_task(command)
        .await
        .map_err(crate::rpc_error)?;
    render_mutation(
        TaskMutationAction::Cancelled,
        id.as_ref(),
        result.task.as_ref(),
        task_status_colors,
        console.color(),
    )
}
