use clap::Args;
use pwf_client::{
    confirmation::ConfirmedRequestError,
    pb::{DeleteTaskStart, delete_task_result},
    task::TaskClient,
};
use pwf_models::settings::TaskStatusColors;

use super::Identifier;

#[derive(Args, Debug)]
pub struct Arguments {
    #[command(flatten)]
    pub(crate) identifier: Identifier,
    /// Skip the removal confirmation (assume yes).
    #[arg(long = "yes", short = 'y')]
    pub(crate) assume_yes: bool,
}

use super::render::{TaskMutationAction, render_mutation};
use crate::{
    confirmation::{CliConfirmationClient, prompt_error},
    console::Console,
};

pub(super) async fn run(
    arguments: &Arguments,
    console: Console,
    task_status_colors: TaskStatusColors,
    client: &TaskClient,
) -> anyhow::Result<String> {
    let id = arguments
        .identifier
        .required(anyhow::anyhow!("--id is required for remove."))?;
    let confirmation_mode = console.confirmation_mode(arguments.assume_yes)?;

    let confirmation_client = CliConfirmationClient::new(console, confirmation_mode);
    let outcome = match client
        .delete_task(
            DeleteTaskStart {
                id: id.to_string(),
                request_id: String::new(),
            },
            confirmation_client,
        )
        .await
    {
        Ok(outcome) => outcome,
        Err(ConfirmedRequestError::Operation(error)) => {
            return Err(anyhow::anyhow!(error.message().to_string()));
        }
        Err(ConfirmedRequestError::Prompt(source)) => {
            return Err(prompt_error("task removal", source));
        }
    };

    match outcome.outcome.as_ref() {
        Some(delete_task_result::Outcome::Deleted(deleted)) => render_mutation(
            TaskMutationAction::Removed,
            id.as_ref(),
            deleted.task.as_ref(),
            task_status_colors,
            console.color(),
        ),
        Some(delete_task_result::Outcome::Aborted(_)) => {
            Ok(format!("# remove {id}: aborted\nnothing deleted."))
        }
        None => Err(anyhow::anyhow!(
            "pwf-server returned an invalid removal outcome"
        )),
    }
}
