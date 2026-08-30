use clap::Args;
use pwf_client::{pb::CancelTaskRequest, task::TaskClient};
use pwf_models::task::TaskReport;

use super::{Identifier, render::render_cancelled};

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
    /// Also spawn a `## Human` review task.
    #[arg(long)]
    pub(crate) review: bool,
}

pub(super) async fn run(arguments: &Arguments, client: &TaskClient) -> anyhow::Result<String> {
    let id = arguments
        .identifier
        .required(anyhow::anyhow!("--id is required for cancel."))?;
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
        review: arguments.review,
        expected_revision: None,
        request_id: String::new(),
    };
    let output = client
        .cancel_task(command)
        .await
        .map_err(crate::rpc_error)?;
    Ok(render_cancelled(
        id.as_ref(),
        output.review_task_id.as_deref(),
    ))
}
