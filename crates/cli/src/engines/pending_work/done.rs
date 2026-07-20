use clap::Args;
use pwf_application::{
    AppDbStore, HandoffDocumentStore, HandoffLedger, IndexEntry, IndexSection, PendingWorkItem,
    pending_work::{
        ProjectRegistry,
        done::{CompletePendingWork, CompletePendingWorkError},
    },
};
use pwf_domain::pending_work::ProjectName;
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
    common::{PendingWorkError, load_configuration},
    render::{
        append_handoff_outcome, done_cancel_reopen_remedy, emit_close_diagnostics,
        emit_created_section, emit_created_section_for_error, render_closed,
    },
};
use crate::config::Config;

pub(super) fn run(arguments: &Arguments) -> Result<String, PendingWorkError> {
    let configuration = load_configuration(&arguments.common)?;
    let store = ObsidianStore::new(configuration.clone());
    run_done(&configuration, &store, arguments)
}

pub(in crate::engines::pending_work) fn run_done<S>(
    cfg: &Config,
    store: &S,
    args: &Arguments,
) -> Result<String, PendingWorkError>
where
    S: AppDbStore<PendingWorkItem>
        + AppDbStore<IndexEntry>
        + AppDbStore<IndexSection>
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
        &projects,
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
