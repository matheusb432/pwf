use clap::Args;
use pwf_application::{
    pending_work::remove_pending_work_item::{
        self, RemovePendingWorkError, RemovePendingWorkItem, RemovePendingWorkItemOk,
    },
    ports::confirmation::{Confirmation, ConfirmationClient},
};
use pwf_infra::obsidian::ObsidianStore;

use super::shared::{CommonArguments, Identifier};

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
    render::{StatusPlacement, render_item_summary, render_removed},
    shared::PendingWorkError,
};
use crate::{
    confirm::{self, DefaultAnswer},
    console::Console,
};

#[derive(Clone, Copy)]
struct CliConfirmationClient {
    console: Console,
    assume_yes: bool,
}

impl ConfirmationClient for CliConfirmationClient {
    fn confirm(&self, confirmation: &Confirmation) -> bool {
        if self.assume_yes {
            return true;
        }
        matches!(
            self.console
                .confirm(&confirmation_message(confirmation), DefaultAnswer::Yes),
            confirm::Confirmation::Accepted | confirm::Confirmation::NonInteractive
        )
    }
}

fn confirmation_message(confirmation: &Confirmation) -> String {
    match confirmation {
        Confirmation::Removal {
            pending_work_identifier,
            project,
            title,
            status,
            note_path,
        } => {
            let summary = render_item_summary(
                pending_work_identifier.as_ref(),
                title,
                Some((*status, StatusPlacement::AfterTitle)),
                false,
            );
            format!(
                "# Confirm task removal\n\n{summary}\n\nproject: {project}\nnote: {}\n\nRemove this pending-work task?",
                note_path.display()
            )
        }
    }
}

pub(super) async fn run(
    arguments: &Arguments,
    console: Console,
    store: &ObsidianStore,
    pool: &sqlx::SqlitePool,
) -> Result<String, PendingWorkError> {
    let id = arguments.identifier.required("remove")?;

    let confirmation_client = CliConfirmationClient {
        console,
        assume_yes: arguments.assume_yes,
    };
    let outcome = remove_pending_work_item::execute(
        &RemovePendingWorkItem { id },
        store,
        pool,
        &confirmation_client,
    )
    .await
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
        RemovePendingWorkError::QueryProject(source) => {
            PendingWorkError::Remove(RemovePendingWorkError::QueryProject(source))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use pwf_application::ports::confirmation::Confirmation;
    use pwf_models::pending_work::{ProjectName, WorkItemId, WorkItemStatus};

    use super::confirmation_message;

    #[test]
    fn confirmation_places_the_item_summary_between_blank_lines() {
        let confirmation = Confirmation::Removal {
            pending_work_identifier: WorkItemId::try_new("PWF-0001").unwrap(),
            project: ProjectName::try_new("pwf").unwrap(),
            title: "stale task".to_string(),
            status: WorkItemStatus::Active,
            note_path: PathBuf::from("/notes/pwf/PWF-0001.md"),
        };

        assert_eq!(
            confirmation_message(&confirmation),
            "# Confirm task removal\n\nPWF-0001 :: stale task (active)\n\nproject: pwf\nnote: /notes/pwf/PWF-0001.md\n\nRemove this pending-work task?"
        );
    }
}
