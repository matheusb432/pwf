use clap::Args;
use pwf_application::{
    ports::clock::Clock,
    task::{
        CloseTaskError,
        cancel_task::{self, CancelTask, CancelTaskError},
    },
};
use pwf_infra::obsidian::ObsidianStore;
use pwf_models::task::{CommitRanges, TaskReport};

use super::{
    Identifier, TaskError,
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
    /// Also spawn a `## Human` review task with prepped git-tools diff commands.
    #[arg(long)]
    pub(crate) review: bool,
}

pub(super) async fn run(
    arguments: &Arguments,
    store: &ObsidianStore,
    pool: &sqlx::SqlitePool,
    clock: &impl Clock,
) -> Result<String, TaskError> {
    let id = arguments.identifier.required("cancel")?;
    let report = arguments
        .report
        .as_deref()
        .ok_or(TaskError::MissingCancelReport)?
        .parse::<TaskReport>()?;
    let command = CancelTask {
        id,
        report,
        commits: CommitRanges::from_inputs(&arguments.commits),
        review: arguments.review,
    };
    let output = cancel_task::execute(&command, store, pool, clock)
        .await
        .inspect_err(|error| {
            if let CancelTaskError::Close(CloseTaskError::ReviewTask(source)) = error {
                emit_created_section_for_error(source);
            }
        })?;
    if let Some(review) = output.review_task.as_ref() {
        emit_created_section(review);
    }
    emit_close_diagnostics(&output);
    Ok(render_closed(&output))
}
