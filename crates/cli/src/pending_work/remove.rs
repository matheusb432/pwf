use clap::Args;
use pwf_application::{
    AppRecordStore, IndexEntry, PendingWorkRecord, PendingWorkRemovalConfirmationClient,
    pending_work::{
        ProjectRegistry,
        remove_pending_work_item::{
            self, RemovalConfirmation, RemovePendingWorkError, RemovePendingWorkItem,
            RemovePendingWorkItemOk,
        },
    },
};
use pwf_infra::obsidian::ObsidianStore;

use super::common::{CommonArguments, Identifier};

#[derive(Args, Debug)]
pub struct Arguments {
    #[command(flatten)]
    pub(crate) identifier: Identifier,
    /// Skip the [Y/n] removal confirmation (assume yes).
    #[arg(long = "yes", short = 'y')]
    pub(crate) assume_yes: bool,
    #[command(flatten)]
    pub(crate) common: CommonArguments,
}

use super::{
    common::PendingWorkError,
    render::{StatusPlacement, render_item_summary, render_removed},
};
use crate::{
    confirm::{Confirmation, DefaultAnswer},
    console::Console,
};

#[derive(Clone, Copy)]
struct CliPendingWorkRemovalConfirmationClient {
    console: Console,
    assume_yes: bool,
}

impl PendingWorkRemovalConfirmationClient for CliPendingWorkRemovalConfirmationClient {
    fn confirm(&self, context: &RemovalConfirmation) -> bool {
        if self.assume_yes {
            return true;
        }
        matches!(
            self.console
                .confirm(&removal_confirmation(context), DefaultAnswer::Yes),
            Confirmation::Accepted | Confirmation::NonInteractive
        )
    }
}

fn removal_confirmation(context: &RemovalConfirmation) -> String {
    let summary = render_item_summary(
        context.pending_work_identifier.as_ref(),
        &context.title,
        Some((context.status, StatusPlacement::AfterTitle)),
        false,
    );
    format!(
        "# Confirm task removal\n\n{summary}\n\nproject: {}\nnote: {}\n\nRemove this pending-work task?",
        context.project,
        context.note_path.display()
    )
}

pub(super) fn run(
    arguments: &Arguments,
    console: Console,
    store: &ObsidianStore,
    projects: &ProjectRegistry,
) -> Result<String, PendingWorkError> {
    run_remove(store, projects, arguments, console)
}

pub(in crate::pending_work) fn run_remove<S>(
    store: &S,
    projects: &ProjectRegistry,
    args: &Arguments,
    console: Console,
) -> Result<String, PendingWorkError>
where
    S: AppRecordStore<PendingWorkRecord> + AppRecordStore<IndexEntry>,
{
    let id = args.identifier.required("remove")?;

    let interaction = CliPendingWorkRemovalConfirmationClient {
        console,
        assume_yes: args.assume_yes,
    };
    let outcome = remove_pending_work_item::execute(
        &RemovePendingWorkItem { id },
        store,
        projects,
        &interaction,
    )
    .map_err(map_remove_error)?;

    match outcome {
        RemovePendingWorkItemOk::Removed(removed) => Ok(render_removed(&removed, console.color())),
        RemovePendingWorkItemOk::Aborted {
            pending_work_identifier,
        } => Ok(format!(
            "# remove {pending_work_identifier} — aborted\nnothing deleted.\n"
        )),
    }
}

fn map_remove_error(error: RemovePendingWorkError) -> PendingWorkError {
    match error {
        RemovePendingWorkError::ItemNotFound { id } => {
            PendingWorkError::Remove(RemovePendingWorkError::ItemNotFound { id })
        }
        RemovePendingWorkError::UnknownPrefix {
            pending_work_identifier,
            prefix,
        } => PendingWorkError::Remove(RemovePendingWorkError::UnknownPrefix {
            pending_work_identifier,
            prefix,
        }),
        RemovePendingWorkError::NoteMissing { path } => {
            PendingWorkError::WorkItemNoteMissing { path: path.into() }
        }
        RemovePendingWorkError::FileModelRequired => PendingWorkError::RemoveRequiresFileModel,
        RemovePendingWorkError::WriteStore(source) => {
            PendingWorkError::Remove(RemovePendingWorkError::WriteStore(source))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use pwf_application::pending_work::remove_pending_work_item::RemovalConfirmation;
    use pwf_models::pending_work::{ProjectName, WorkItemId, WorkItemStatus};

    use super::removal_confirmation;

    #[test]
    fn confirmation_places_the_item_summary_between_blank_lines() {
        let context = RemovalConfirmation {
            pending_work_identifier: WorkItemId::try_new("PWF-0001").unwrap(),
            project: ProjectName::try_new("pwf").unwrap(),
            title: "stale task".to_string(),
            status: WorkItemStatus::Active,
            note_path: PathBuf::from("/notes/pwf/PWF-0001.md"),
        };

        assert_eq!(
            removal_confirmation(&context),
            "# Confirm task removal\n\nPWF-0001 :: stale task (active)\n\nproject: pwf\nnote: /notes/pwf/PWF-0001.md\n\nRemove this pending-work task?"
        );
    }
}
