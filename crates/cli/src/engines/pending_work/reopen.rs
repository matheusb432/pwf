use clap::Args;
use pwf_application::{
    AppDbStore, HandoffDocumentStore, HandoffLedger, IndexEntry, PendingWorkItem,
    pending_work::{
        ProjectRegistry,
        reopen::{ReopenPendingWork, ReopenPendingWorkError},
    },
};
use pwf_domain::pending_work::ProjectName;
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
    common::{PendingWorkError, load_configuration},
    render::{append_handoff_outcome, done_cancel_reopen_remedy, render_reopened},
};
use crate::config::Config;

pub(super) fn run(arguments: &Arguments) -> Result<String, PendingWorkError> {
    let configuration = load_configuration(&arguments.common)?;
    let store = ObsidianStore::new(configuration.clone());
    run_reopen(&configuration, &store, arguments)
}

pub(in crate::engines::pending_work) fn run_reopen<S>(
    cfg: &Config,
    store: &S,
    args: &Arguments,
) -> Result<String, PendingWorkError>
where
    S: AppDbStore<PendingWorkItem>
        + AppDbStore<IndexEntry>
        + HandoffDocumentStore
        + AppDbStore<HandoffLedger>,
{
    let projects = ProjectRegistry::new(cfg.projects.iter().map(|(name, repository)| {
        (
            ProjectName::try_new(name).expect("configured project is non-empty"),
            Some(repository.clone()),
            cfg.prefixes
                .get(name)
                .map(|prefix| prefix.to_ascii_uppercase()),
        )
    }));
    let id = args.identifier.required("reopen")?;
    let outcome =
        pwf_application::pending_work::reopen::execute(&ReopenPendingWork { id }, store, &projects)
            .map_err(map_reopen_error)?;
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
