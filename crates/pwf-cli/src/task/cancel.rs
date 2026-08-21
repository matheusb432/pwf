use clap::Args;
use pwf_client::{task::TaskClient, v1::CancelTaskRequest};
use pwf_models::task::TaskReport;

use super::{
    Identifier,
    render::{
        emit_close_diagnostics, emit_created_section, emit_created_section_for_error, render_closed,
    },
};

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
    };
    let output = client.cancel_task(command).await.map_err(|error| {
        emit_created_section_for_error(&error);
        crate::rpc_error(error)
    })?;
    if let Some(review) = output.review_task.as_ref() {
        emit_created_section(review);
    }
    emit_close_diagnostics(&output);
    Ok(render_closed(&output))
}
