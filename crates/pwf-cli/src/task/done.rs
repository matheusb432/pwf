use clap::Args;
use pwf_client::{task::TaskClient, v1::CompleteTaskRequest};
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

use super::render::{
    emit_complete_diagnostics, emit_created_section, emit_created_section_for_error,
    render_completed,
};

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
        })
        .await
        .map_err(|error| {
            emit_created_section_for_error(&error);
            crate::rpc_error(error)
        })?;
    if let Some(review) = output.review_task.as_ref() {
        emit_created_section(review);
    }
    emit_complete_diagnostics(&output);
    Ok(render_completed(&output))
}
