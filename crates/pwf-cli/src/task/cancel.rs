use clap::Args;
use pwf_application::{
    ports::clock::Clock,
    task::cancel_task::{self, CancelTaskError},
};
use pwf_infra::obsidian::ObsidianStore;
use pwf_models::task::{CommitRanges, TaskReport};
use pwf_wire::task::{CancelTask, CancelTaskApiError, CloseTaskApiError};

use super::{
    Identifier, map_close_task_error, map_resolve_task_project_error,
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

pub(super) async fn run(
    arguments: &Arguments,
    store: &ObsidianStore,
    pool: &sqlx::SqlitePool,
    clock: &impl Clock,
) -> Result<String, CancelTaskApiError> {
    let id = arguments
        .identifier
        .required(CancelTaskApiError::MissingId)?;
    let report = arguments
        .report
        .as_deref()
        .ok_or(CancelTaskApiError::MissingReport)?
        .parse::<TaskReport>()
        .map_err(|error| CancelTaskApiError::InvalidReport {
            message: error.to_string(),
        })?;
    let command = CancelTask {
        id,
        report,
        commits: CommitRanges::from_inputs(&arguments.commits),
        review: arguments.review,
    };
    let output = cancel_task::execute(&command, store, pool, clock)
        .await
        .map_err(map_error)
        .inspect_err(|error| {
            if let CancelTaskApiError::Close(CloseTaskApiError::ReviewTask(source)) = error {
                emit_created_section_for_error(source);
            }
        })?;
    if let Some(review) = output.review_task.as_ref() {
        emit_created_section(review);
    }
    emit_close_diagnostics(&output);
    Ok(render_closed(&output))
}

fn map_error(error: CancelTaskError) -> CancelTaskApiError {
    CancelTaskApiError::Close(match error {
        CancelTaskError::ResolveProject(error) => map_resolve_task_project_error(error).into(),
        CancelTaskError::Close(error) => map_close_task_error(error),
    })
}
