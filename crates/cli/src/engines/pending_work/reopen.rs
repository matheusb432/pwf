use clap::Args;
use pwf_application::{
    AppRecordStore, HandoffDocumentStore, HandoffLedger, IndexEntry, PendingWorkItem,
    pending_work::{
        ProjectRegistry,
        reopen::{self, ReopenPendingWork, ReopenPendingWorkError},
    },
};
use pwf_infra::obsidian::ObsidianStore;

use super::common::{CommonArguments, Identifier};

#[derive(Args, Debug)]
pub struct Arguments {
    #[command(flatten)]
    pub(crate) identifier: Identifier,
    #[command(flatten)]
    pub(crate) common: CommonArguments,
}

use super::{
    common::PendingWorkError,
    render::{append_handoff_outcome, done_cancel_reopen_remedy, render_reopened},
};

pub(super) fn run(
    arguments: &Arguments,
    store: &ObsidianStore,
    projects: &ProjectRegistry,
) -> Result<String, PendingWorkError> {
    run_reopen(store, projects, arguments)
}

pub(in crate::engines::pending_work) fn run_reopen<S>(
    store: &S,
    projects: &ProjectRegistry,
    args: &Arguments,
) -> Result<String, PendingWorkError>
where
    S: AppRecordStore<PendingWorkItem>
        + AppRecordStore<IndexEntry>
        + HandoffDocumentStore
        + AppRecordStore<HandoffLedger>,
{
    let id = args.identifier.required("reopen")?;
    let outcome =
        reopen::execute(&ReopenPendingWork { id }, store, projects).map_err(map_reopen_error)?;
    Ok(append_handoff_outcome(
        render_reopened(&outcome),
        &outcome.handoff,
        "reopened",
    ))
}

fn map_reopen_error(error: ReopenPendingWorkError) -> PendingWorkError {
    match error {
        ReopenPendingWorkError::ItemNotFound { id } => PendingWorkError::ItemNotFound { id },
        ReopenPendingWorkError::UnknownPrefix {
            pending_work_identifier,
            prefix,
        } => PendingWorkError::Reopen(ReopenPendingWorkError::UnknownPrefix {
            pending_work_identifier,
            prefix,
        }),
        ReopenPendingWorkError::WriteStore(source) => {
            PendingWorkError::Reopen(ReopenPendingWorkError::WriteStore(source))
        }
        ReopenPendingWorkError::HandoffPreflight(source) => {
            PendingWorkError::HandoffLifecycle(source)
        }
        ReopenPendingWorkError::HandoffAfterPendingWork {
            pending_work_identifier,
            source,
        } => PendingWorkError::HandoffLifecycleAfterMutation {
            id: pending_work_identifier.to_string(),
            source,
            remedy: done_cancel_reopen_remedy(pending_work_identifier.as_ref()),
        },
    }
}
