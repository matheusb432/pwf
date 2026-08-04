use clap::Args;
use pwf_application::{
    ports::clock::Clock,
    task::complete_task::{self, CloseTaskError, CompleteTask, CompleteTaskError},
};
use pwf_infra::obsidian::ObsidianStore;

use super::shared::{CommonArguments, Identifier};

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
    /// Also spawn a `## Human` review task with prepped git-tools diff commands.
    #[arg(long)]
    pub(crate) review: bool,
    #[command(flatten)]
    pub(crate) common: CommonArguments,
}

use super::{
    render::{
        emit_close_diagnostics, emit_created_section, emit_created_section_for_error, render_closed,
    },
    shared::TaskError,
};

pub(super) async fn run(
    arguments: &Arguments,
    store: &ObsidianStore,
    pool: &sqlx::SqlitePool,
    clock: &impl Clock,
) -> Result<String, TaskError> {
    let id = arguments.identifier.required("done")?;
    let output = complete_task::execute(
        &CompleteTask {
            id,
            date: arguments.common.date.clone(),
            report: arguments.report.clone(),
            commits: arguments.commits.clone(),
            review: arguments.review,
        },
        store,
        pool,
        clock,
    )
    .await
    .inspect_err(|error| {
        if let CompleteTaskError::Close(CloseTaskError::ReviewTask(source)) = error {
            emit_created_section_for_error(source);
        }
    })?;
    if let Some(review) = output.review_task.as_ref() {
        emit_created_section(review);
    }
    emit_close_diagnostics(&output);
    Ok(render_closed(&output))
}
