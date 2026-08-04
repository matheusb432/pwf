use clap::Args;
use pwf_application::{
    ports::clock::Clock,
    task::complete_task::{self, CompleteTask, CompleteTaskError},
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
    .map_err(map_complete_error)?;
    if let Some(review) = output.review_task.as_ref() {
        emit_created_section(review);
    }
    emit_close_diagnostics(&output);
    Ok(render_closed(&output))
}

fn map_complete_error(error: CompleteTaskError) -> TaskError {
    match error {
        CompleteTaskError::TaskNotFound { id } => TaskError::TaskNotFound { id },
        CompleteTaskError::UnknownProjectId {
            task_id,
            project_id,
        } => TaskError::Complete(CompleteTaskError::UnknownProjectId {
            task_id,
            project_id,
        }),
        CompleteTaskError::EmptyReport => TaskError::EmptyReport,
        CompleteTaskError::WriteStore(source) => {
            TaskError::Complete(CompleteTaskError::WriteStore(source))
        }
        CompleteTaskError::ReviewTask(source) => {
            emit_created_section_for_error(&source);
            TaskError::Complete(CompleteTaskError::ReviewTask(source))
        }
        CompleteTaskError::QueryProject(source) => {
            TaskError::Complete(CompleteTaskError::QueryProject(source))
        }
    }
}
