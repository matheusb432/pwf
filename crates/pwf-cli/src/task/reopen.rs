use clap::Args;
use pwf_application::task::reopen_task::{self, ReopenTaskError};
use pwf_infra::obsidian::ObsidianStore;
use pwf_wire::task::{ReopenTask, ReopenTaskApiError};

use super::{Identifier, map_resolve_task_project_error};

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
    store: &ObsidianStore,
    pool: &sqlx::SqlitePool,
) -> anyhow::Result<String> {
    let id = arguments
        .identifier
        .required(ReopenTaskApiError::MissingId)?;
    let confirmation_mode = console.confirmation_mode(arguments.assume_yes)?;
    let confirmation_client = CliConfirmationClient::new(console, confirmation_mode);
    let outcome = reopen_task::execute(&ReopenTask { id }, store, pool, &confirmation_client)
        .await
        .map_err(map_error)?;
    if let Some(source) = confirmation_client.into_prompt_error() {
        return Err(prompt_error("task reopening", source));
    }
    Ok(render_reopened(&outcome))
}

fn map_error(error: ReopenTaskError) -> ReopenTaskApiError {
    match error {
        ReopenTaskError::TaskNotFound { id } => ReopenTaskApiError::TaskNotFound { id },
        ReopenTaskError::ResolveProject(error) => map_resolve_task_project_error(error).into(),
        ReopenTaskError::WriteStore(source) => ReopenTaskApiError::Unexpected {
            message: source.to_string(),
        },
    }
}
