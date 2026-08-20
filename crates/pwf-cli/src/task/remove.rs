use clap::Args;
use pwf_application::task::remove_task::{self, RemoveTaskError};
use pwf_infra::obsidian::ObsidianStore;
use pwf_wire::task::{RemoveTask, RemoveTaskApiError, RemovedTaskOutcome};

use super::{Identifier, map_resolve_task_project_error};

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
    store: &ObsidianStore,
    pool: &sqlx::SqlitePool,
) -> anyhow::Result<String> {
    let id = arguments
        .identifier
        .required(RemoveTaskApiError::MissingId)?;
    let confirmation_mode = console.confirmation_mode(arguments.assume_yes)?;

    let confirmation_client = CliConfirmationClient::new(console, confirmation_mode);
    let outcome = remove_task::execute(&RemoveTask { id }, store, pool, &confirmation_client)
        .await
        .map_err(map_error)?;
    if let Some(source) = confirmation_client.into_prompt_error() {
        return Err(prompt_error("task removal", source));
    }

    match outcome {
        RemovedTaskOutcome::Removed(removed) => Ok(render_removed(&removed, console.color())),
        RemovedTaskOutcome::Aborted { task_id } => {
            Ok(format!("# remove {task_id}: aborted\nnothing deleted.\n"))
        }
    }
}

fn map_error(error: RemoveTaskError) -> RemoveTaskApiError {
    match error {
        RemoveTaskError::TaskNotFound { id } => RemoveTaskApiError::TaskNotFound { id },
        RemoveTaskError::ResolveProject(error) => map_resolve_task_project_error(error).into(),
        RemoveTaskError::NoteMissing { path } => RemoveTaskApiError::NoteMissing { path },
        RemoveTaskError::InvalidTitle { id, source } => RemoveTaskApiError::InvalidTitle {
            id,
            reason: source.to_string(),
        },
        RemoveTaskError::HasDependents { target, dependents } => {
            RemoveTaskApiError::HasDependents { target, dependents }
        }
        RemoveTaskError::MalformedBlockedBy {
            task,
            path,
            raw,
            reason,
        } => RemoveTaskApiError::MalformedBlockedBy {
            task,
            path,
            raw,
            reason,
        },
        RemoveTaskError::WriteStore(source) | RemoveTaskError::ReadDependents(source) => {
            RemoveTaskApiError::Unexpected {
                message: source.to_string(),
            }
        }
    }
}
