use clap::Args;
use pwf_application::{
    pending_work::cancel_pending_work::{self, CancelPendingWork, CancelPendingWorkError},
    ports::clock::Clock,
};
use pwf_infra::obsidian::ObsidianStore;

use super::{
    render::{
        emit_close_diagnostics, emit_created_section, emit_created_section_for_error, render_closed,
    },
    shared::{CommonArguments, Identifier, PendingWorkError},
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
    #[command(flatten)]
    pub(crate) common: CommonArguments,
}

pub(super) async fn run(
    arguments: &Arguments,
    store: &ObsidianStore,
    pool: &sqlx::SqlitePool,
    clock: &impl Clock,
) -> Result<String, PendingWorkError> {
    let id = arguments.identifier.required("cancel")?;
    let report = arguments
        .report
        .clone()
        .ok_or(PendingWorkError::MissingCancelReport)?;
    let command = CancelPendingWork::new(
        id,
        arguments.common.date.clone(),
        report,
        arguments.commits.clone(),
        arguments.review,
    );
    let output = cancel_pending_work::execute(&command, store, pool, clock)
        .await
        .map_err(map_error)?;
    if let Some(review) = output.review_item.as_ref() {
        emit_created_section(review);
    }
    emit_close_diagnostics(&output);
    Ok(render_closed(&output))
}

fn map_error(error: CancelPendingWorkError) -> PendingWorkError {
    match error {
        CancelPendingWorkError::EmptyReport => PendingWorkError::EmptyReport,
        CancelPendingWorkError::ItemNotFound { id } => PendingWorkError::ItemNotFound { id },
        CancelPendingWorkError::UnknownPrefix {
            pending_work_identifier,
            prefix,
        } => PendingWorkError::Cancel(CancelPendingWorkError::UnknownPrefix {
            pending_work_identifier,
            prefix,
        }),
        CancelPendingWorkError::WriteStore(source) => {
            PendingWorkError::Cancel(CancelPendingWorkError::WriteStore(source))
        }
        CancelPendingWorkError::ReviewTask(source) => {
            emit_created_section_for_error(&source);
            PendingWorkError::Cancel(CancelPendingWorkError::ReviewTask(source))
        }
        CancelPendingWorkError::QueryProject(source) => {
            PendingWorkError::Cancel(CancelPendingWorkError::QueryProject(source))
        }
    }
}
