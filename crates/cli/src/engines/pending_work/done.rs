use clap::Args;
use pwf_application::{
    AppRecordStore, HandoffDocumentStore, HandoffLedger, IndexEntry, IndexSection, PendingWorkItem,
    pending_work::{
        ProjectRegistry,
        done::{CompletePendingWork, CompletePendingWorkError},
    },
};
use pwf_infra::obsidian::ObsidianStore;

use super::common::{CommonArguments, Identifier};

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
    common::PendingWorkError,
    render::{
        append_handoff_outcome, done_cancel_reopen_remedy, emit_close_diagnostics,
        emit_created_section, emit_created_section_for_error, render_closed,
    },
};

pub(super) fn run(
    arguments: &Arguments,
    store: &ObsidianStore,
    projects: &ProjectRegistry,
) -> Result<String, PendingWorkError> {
    run_done(store, projects, arguments)
}

pub(in crate::engines::pending_work) fn run_done<S>(
    store: &S,
    projects: &ProjectRegistry,
    args: &Arguments,
) -> Result<String, PendingWorkError>
where
    S: AppRecordStore<PendingWorkItem>
        + AppRecordStore<IndexEntry>
        + AppRecordStore<IndexSection>
        + HandoffDocumentStore
        + AppRecordStore<HandoffLedger>,
{
    let id = args.identifier.required("done")?;
    let completed = pwf_core::date::stamp_date(args.common.date.as_deref());
    let output = pwf_application::pending_work::done::execute(
        &CompletePendingWork {
            id,
            completed,
            report: args.report.clone(),
            commits: args.commits.clone(),
            review: args.review,
        },
        store,
        projects,
    )
    .map_err(map_complete_error)?;
    if let Some(review) = output.review_item.as_ref() {
        emit_created_section(review);
    }
    emit_close_diagnostics(&output);
    Ok(append_handoff_outcome(
        render_closed(&output),
        &output.handoff,
        "archived",
    ))
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
        CompletePendingWorkError::HandoffPreflight(source) => {
            PendingWorkError::HandoffLifecycle(source)
        }
        CompletePendingWorkError::HandoffAfterPendingWork {
            pending_work_identifier,
            completed,
            source,
        } => {
            if let Some(review) = completed.review_item.as_ref() {
                emit_created_section(review);
            }
            emit_close_diagnostics(&completed);
            PendingWorkError::HandoffLifecycleAfterMutation {
                id: pending_work_identifier.to_string(),
                source,
                remedy: done_cancel_reopen_remedy(pending_work_identifier.as_ref()),
            }
        }
    }
}
