use clap::Args;
use pwf_client::{confirmation::ConfirmedRequestError, pb::ReopenTaskStart, task::TaskClient};
use pwf_models::settings::TaskStatusColors;

use super::Identifier;

#[derive(Args, Debug)]
pub struct Arguments {
    #[command(flatten)]
    pub(crate) identifier: Identifier,
    /// Skip the reopening confirmation (assume yes).
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
    let id = arguments.identifier.id();
    let confirmation_mode = console.confirmation_mode(arguments.assume_yes)?;
    let confirmation_client = CliConfirmationClient::new(console, confirmation_mode);
    let outcome = match client
        .reopen_task(
            ReopenTaskStart {
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
            return Err(prompt_error("task reopening", source));
        }
    };
    match outcome.outcome {
        Some(pwf_client::pb::reopen_task_result::Outcome::Reopened(result)) => render_mutation(
            TaskMutationAction::Reopened,
            id.as_ref(),
            result.task.as_ref(),
            task_status_colors,
            console.color(),
        ),
        Some(pwf_client::pb::reopen_task_result::Outcome::AlreadyActive(result)) => {
            render_mutation(
                TaskMutationAction::Skipped,
                id.as_ref(),
                result.task.as_ref(),
                task_status_colors,
                console.color(),
            )
        }
        Some(pwf_client::pb::reopen_task_result::Outcome::Aborted(_)) => {
            Ok(format!("# reopen {id}: aborted\nnothing changed."))
        }
        None => Err(anyhow::anyhow!(
            "pwf-server returned an invalid reopening outcome"
        )),
    }
}
