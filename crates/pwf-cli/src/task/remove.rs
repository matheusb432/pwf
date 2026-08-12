use clap::Args;
use pwf_application::{
    ports::confirmation::{Confirmation, ConfirmationClient},
    task::remove_task::{self, RemoveTask},
};
use pwf_infra::obsidian::ObsidianStore;
use pwf_wire::task::RemovedTaskOutcome;

use super::Identifier;

#[derive(Args, Debug)]
pub struct Arguments {
    #[command(flatten)]
    pub(crate) identifier: Identifier,
    /// Skip the [Y/n] removal confirmation (assume yes).
    #[arg(long = "yes", short = 'y')]
    pub(crate) assume_yes: bool,
}

use super::{
    TaskError,
    render::{StatusPlacement, render_removed, render_task_summary},
};
use crate::{confirm, console::Console};

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
            self.console.confirm(&confirmation_message(confirmation)),
            confirm::Confirmation::Accepted | confirm::Confirmation::NonInteractive
        )
    }
}

fn confirmation_message(confirmation: &Confirmation) -> String {
    match confirmation {
        Confirmation::Removal {
            task_identifier,
            project,
            title,
            status,
            note_path,
        } => {
            let summary = render_task_summary(
                task_identifier.as_ref(),
                title.as_ref(),
                Some((*status, StatusPlacement::AfterTitle)),
                false,
            );
            format!(
                "# Confirm task removal\n\n{summary}\n\nproject: {project}\nnote: {note_path}\n\nRemove this task?"
            )
        }
    }
}

pub(super) async fn run(
    arguments: &Arguments,
    console: Console,
    store: &ObsidianStore,
    pool: &sqlx::SqlitePool,
) -> Result<String, TaskError> {
    let id = arguments.identifier.required("remove")?;

    let confirmation_client = CliConfirmationClient {
        console,
        assume_yes: arguments.assume_yes,
    };
    let outcome =
        remove_task::execute(&RemoveTask { id }, store, pool, &confirmation_client).await?;

    match outcome {
        RemovedTaskOutcome::Removed(removed) => Ok(render_removed(&removed, console.color())),
        RemovedTaskOutcome::Aborted { task_id } => {
            Ok(format!("# remove {task_id} — aborted\nnothing deleted.\n"))
        }
    }
}

#[cfg(test)]
mod tests {
    use pwf_application::ports::confirmation::Confirmation;
    use pwf_models::{
        project::ProjectName,
        task::{TaskId, TaskStatus, TaskTitle},
    };
    use pwf_wire::task::TaskNotePath;

    use super::confirmation_message;

    #[test]
    fn confirmation_places_the_task_summary_between_blank_lines() {
        let confirmation = Confirmation::Removal {
            task_identifier: TaskId::try_new("PWF-0001").unwrap(),
            project: ProjectName::try_new("pwf").unwrap(),
            title: TaskTitle::try_new("stale task").unwrap(),
            status: TaskStatus::Active,
            note_path: TaskNotePath::new("/notes/pwf/PWF-0001.md".into()),
        };

        assert_eq!(
            confirmation_message(&confirmation),
            "# Confirm task removal\n\nPWF-0001 :: stale task (active)\n\nproject: pwf\nnote: /notes/pwf/PWF-0001.md\n\nRemove this task?"
        );
    }
}
