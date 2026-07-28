use clap::Args;
use pwf_application::{
    AppRecordStore, IndexEntry, PendingWorkItem,
    pending_work::{
        ProjectRegistry,
        remove_pending_work_item::{
            self, RemovalConfirmation, RemovalInteraction, RemovePendingWorkError,
            RemovePendingWorkItem, RemovePendingWorkItemOk,
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

use super::{common::PendingWorkError, render::render_removed};
use crate::{
    confirm::{Confirmation, DefaultAnswer},
    confirm_prompt::{ConfirmationPrompt, Field},
    console::Console,
};

#[derive(Clone, Copy)]
struct CliRemovalInteraction {
    console: Console,
    assume_yes: bool,
}

impl RemovalInteraction for CliRemovalInteraction {
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
    let fields = [
        Field::new("task_id", context.pending_work_identifier.to_string()),
        Field::new("project", context.project.to_string()),
        Field::new("title", context.title.clone()),
        Field::new("note", context.note_path.display().to_string()),
    ];
    ConfirmationPrompt::new(
        "Confirm task removal",
        &fields,
        "Remove this pending-work task?",
    )
    .to_string()
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
    S: AppRecordStore<PendingWorkItem> + AppRecordStore<IndexEntry>,
{
    let id = args.identifier.required("remove")?;

    let interaction = CliRemovalInteraction {
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
        RemovePendingWorkError::ItemNotFound { id } => PendingWorkError::ItemNotFound { id },
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
