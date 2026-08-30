use clap::Args;
use pwf_client::{
    task::{ConfirmedRequestError, TaskClient},
    v1::ReopenTaskStart,
};

use super::Identifier;

#[derive(Args, Debug)]
pub struct Arguments {
    #[command(flatten)]
    pub(crate) identifier: Identifier,
    /// Skip the reopening confirmation (assume yes).
    #[arg(long = "yes", short = 'y')]
    pub(crate) assume_yes: bool,
}

use super::render::render_reopened;
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
        .required(anyhow::anyhow!("--id is required for reopen."))?;
    let confirmation_mode = console.confirmation_mode(arguments.assume_yes)?;
    let confirmation_client = CliConfirmationClient::new(console, confirmation_mode);
    let outcome = match client
        .reopen_task(ReopenTaskStart { id: id.to_string() }, confirmation_client)
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
    Ok(render_reopened(id.as_ref(), &outcome))
}
