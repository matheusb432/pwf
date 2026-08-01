use clap::Args;
use pwf_application::{
    pending_work::{
        ProjectRegistry,
        complete_pending_work::{self, CompletePendingWork, CompletePendingWorkError},
    },
    ports::clock::Clock,
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
    shared::PendingWorkError,
};

pub(super) fn run(
    arguments: &Arguments,
    store: &ObsidianStore,
    projects: &ProjectRegistry,
    clock: &impl Clock,
) -> Result<String, PendingWorkError> {
    let id = arguments.identifier.required("done")?;
    let output = complete_pending_work::execute(
        &CompletePendingWork {
            id,
            date: arguments.common.date.clone(),
            report: arguments.report.clone(),
            commits: arguments.commits.clone(),
            review: arguments.review,
        },
        store,
        projects,
        clock,
    )
    .map_err(map_complete_error)?;
    if let Some(review) = output.review_item.as_ref() {
        emit_created_section(review);
    }
    emit_close_diagnostics(&output);
    Ok(render_closed(&output))
}

fn map_complete_error(error: CompletePendingWorkError) -> PendingWorkError {
    match error {
        CompletePendingWorkError::ItemNotFound { id } => PendingWorkError::ItemNotFound { id },
        CompletePendingWorkError::UnknownPrefix {
            pending_work_identifier,
            prefix,
        } => PendingWorkError::Complete(CompletePendingWorkError::UnknownPrefix {
            pending_work_identifier,
            prefix,
        }),
        CompletePendingWorkError::EmptyReport => PendingWorkError::EmptyReport,
        CompletePendingWorkError::WriteStore(source) => {
            PendingWorkError::Complete(CompletePendingWorkError::WriteStore(source))
        }
        CompletePendingWorkError::ReviewTask(source) => {
            emit_created_section_for_error(&source);
            PendingWorkError::Complete(CompletePendingWorkError::ReviewTask(source))
        }
    }
}
