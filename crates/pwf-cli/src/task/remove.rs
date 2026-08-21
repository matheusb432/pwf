use clap::Args;
use pwf_client::{
    task::{ConfirmedRequestError, TaskClient},
    v1::{RemoveTaskRequest, RemovedTaskOutcomeKind},
};

use super::Identifier;

#[derive(Args, Debug)]
pub struct Arguments {
    #[command(flatten)]
    pub(crate) identifier: Identifier,
    /// Skip the removal confirmation (assume yes).
    #[arg(long = "yes", short = 'y')]
    pub(crate) assume_yes: bool,
}

use super::render::render_removed;
use crate::{
    confirmation::{CliConfirmationClient, prompt_error},
    console::Console,
};

pub(super) async fn run(
    arguments: &Arguments,
    console: Console,
    client: &TaskClient,
) -> anyhow::Result<String> {
    let id = arguments
        .identifier
        .required(anyhow::anyhow!("--id is required for remove."))?;
    let confirmation_mode = console.confirmation_mode(arguments.assume_yes)?;

    let confirmation_client = CliConfirmationClient::new(console, confirmation_mode);
    let outcome = match client
        .remove_task(
            RemoveTaskRequest { id: id.to_string() },
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

    match RemovedTaskOutcomeKind::try_from(outcome.outcome).ok() {
        Some(RemovedTaskOutcomeKind::Removed) => outcome
            .removed
            .as_ref()
            .map(|removed| render_removed(removed, console.color()))
            .ok_or_else(|| anyhow::anyhow!("pwf-server returned removal without task details")),
        Some(RemovedTaskOutcomeKind::Aborted) => Ok(format!(
            "# remove {}: aborted\nnothing deleted.\n",
            outcome.task_id
        )),
        Some(RemovedTaskOutcomeKind::Unspecified) | None => Err(anyhow::anyhow!(
            "pwf-server returned an invalid removal outcome"
        )),
    }
}
