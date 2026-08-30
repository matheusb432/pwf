use clap::Args;
use pwf_client::{pb::CompleteTaskRequest, task::TaskClient};
use pwf_models::task::TaskReport;

use super::Identifier;

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
    /// Also spawn a `## Human` review task.
    #[arg(long)]
    pub(crate) review: bool,
}

use super::render::render_completed;

pub(super) async fn run(arguments: &Arguments, client: &TaskClient) -> anyhow::Result<String> {
    let id = arguments
        .identifier
        .required(anyhow::anyhow!("--id is required for done."))?;
    let output = client
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
            review: arguments.review,
            expected_revision: None,
            request_id: String::new(),
        })
        .await
        .map_err(crate::rpc_error)?;
    Ok(render_completed(
        id.as_ref(),
        output.review_task_id.as_deref(),
    ))
}
