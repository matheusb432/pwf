use clap::Args;
use pwf_client::{confirmation::ConfirmedRequestError, pb::ActivateTaskStart, task::TaskClient};
use pwf_models::settings::TaskStatusColors;

use super::Identifier;

#[derive(Args, Debug)]
pub struct Arguments {
    #[command(flatten)]
    pub(crate) identifier: Identifier,
    /// Confirm removal of a closed task's completion data without prompting.
    #[arg(long = "yes", short = 'y')]
    pub(crate) assume_yes: bool,
}

use super::render::{TaskMutationAction, render_mutation};
use crate::{
    confirmation::{CliConfirmationClient, ConfirmationMode, prompt_error},
    console::Console,
};

pub(super) async fn run(
    arguments: &Arguments,
    console: Console,
    task_status_colors: TaskStatusColors,
    client: &TaskClient,
) -> anyhow::Result<String> {
    let id = arguments.identifier.id();
    let confirmation_mode = if arguments.assume_yes {
        ConfirmationMode::AssumeYes
    } else {
        ConfirmationMode::Prompt
    };
    let confirmation_client = CliConfirmationClient::new(console, confirmation_mode);
    let outcome = match client
        .activate_task(
            ActivateTaskStart { id: id.to_string() },
            confirmation_client,
        )
        .await
    {
        Ok(outcome) => outcome,
        Err(ConfirmedRequestError::Operation(error)) => {
            return Err(anyhow::anyhow!(error.message().to_string()));
        }
        Err(ConfirmedRequestError::Prompt(source)) => {
            return Err(prompt_error("task activation", source));
        }
    };
    match outcome.outcome {
        Some(pwf_client::pb::activate_task_result::Outcome::Activated(result)) => render_mutation(
            TaskMutationAction::Activated,
            id.as_ref(),
            result.task.as_ref(),
            task_status_colors,
            console.color(),
        ),
        Some(pwf_client::pb::activate_task_result::Outcome::AlreadyActive(result)) => {
            render_mutation(
                TaskMutationAction::AlreadyActive,
                id.as_ref(),
                result.task.as_ref(),
                task_status_colors,
                console.color(),
            )
        }
        Some(pwf_client::pb::activate_task_result::Outcome::Aborted(_)) => {
            Ok(format!("# activate {id}: aborted\nnothing changed."))
        }
        None => Err(anyhow::anyhow!(
            "pwf-server returned an invalid activation outcome"
        )),
    }
}
