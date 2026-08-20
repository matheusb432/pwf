use clap::Args;
use pwf_application::{
    ports::clock::Clock,
    task::complete_task::{self, CompleteTaskError},
};
use pwf_infra::obsidian::ObsidianStore;
use pwf_models::task::{CommitRanges, TaskReport};
use pwf_wire::task::{CloseTaskApiError, CompleteTask, CompleteTaskApiError};

use super::{Identifier, map_close_task_error, map_resolve_task_project_error};

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
    emit_close_diagnostics, emit_created_section, emit_created_section_for_error, render_closed,
};

pub(super) async fn run(
    arguments: &Arguments,
    store: &ObsidianStore,
    pool: &sqlx::SqlitePool,
    clock: &impl Clock,
) -> Result<String, CompleteTaskApiError> {
    let id = arguments
        .identifier
        .required(CompleteTaskApiError::MissingId)?;
    let output = complete_task::execute(
        &CompleteTask {
            id,
            report: arguments
                .report
                .as_deref()
                .map(str::parse::<TaskReport>)
                .transpose()
                .map_err(|error| CompleteTaskApiError::InvalidReport {
                    message: error.to_string(),
                })?,
            commits: CommitRanges::from_inputs(&arguments.commits),
            review: arguments.review,
        },
        store,
        pool,
        clock,
    )
    .await
    .map_err(map_error)
    .inspect_err(|error| {
        if let CompleteTaskApiError::Close(CloseTaskApiError::ReviewTask(source)) = error {
            emit_created_section_for_error(source);
        }
    })?;
    if let Some(review) = output.review_task.as_ref() {
        emit_created_section(review);
    }
    emit_close_diagnostics(&output);
    Ok(render_closed(&output))
}

fn map_error(error: CompleteTaskError) -> CompleteTaskApiError {
    CompleteTaskApiError::Close(match error {
        CompleteTaskError::ResolveProject(error) => map_resolve_task_project_error(error).into(),
        CompleteTaskError::Close(error) => map_close_task_error(error),
    })
}
