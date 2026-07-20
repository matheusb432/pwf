use clap::Args;
use pwf_application::pending_work::{
    ProjectRegistry,
    cancel::{CancelPendingWork, CancelPendingWorkError},
};
use pwf_domain::pending_work::ProjectName;
use pwf_infra::obsidian::ObsidianStore;

use super::{
    common::{CommonArguments, Identifier, PendingWorkError, load_configuration},
    render::{
        append_handoff_outcome, done_cancel_reopen_remedy, emit_close_diagnostics,
        emit_created_section, emit_created_section_for_error, render_closed,
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
    #[command(flatten)]
    pub(crate) common: CommonArguments,
}

pub(super) fn run(arguments: &Arguments) -> Result<String, PendingWorkError> {
    let configuration = load_configuration(&arguments.common)?;
    let projects = ProjectRegistry::new(configuration.projects.iter().map(|(name, repository)| {
        (
            ProjectName::try_new(name).expect("configured project is non-empty"),
            Some(repository.clone()),
            configuration
                .prefixes
                .get(name)
                .map(|prefix| prefix.to_ascii_uppercase()),
        )
    }));
    let store = ObsidianStore::new(configuration);
    let id = arguments.identifier.required("cancel")?;
    let report = arguments
        .report
        .clone()
        .ok_or(PendingWorkError::MissingCancelReport)?;
    let command = CancelPendingWork::new(
        id,
        pwf_core::date::stamp_date(arguments.common.date.as_deref()),
        report,
        arguments.commits.clone(),
        arguments.review,
    );
    let output = pwf_application::pending_work::cancel::execute(&command, &store, &projects)
        .map_err(map_error)?;
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
        CancelPendingWorkError::HandoffPreflight(source) => {
            PendingWorkError::HandoffLifecycle(source)
        }
        CancelPendingWorkError::HandoffAfterPendingWork {
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
