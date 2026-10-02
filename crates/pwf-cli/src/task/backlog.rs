use clap::Args;
use pwf_client::{pb, task::TaskClient};
use pwf_models::settings::UserSettings;

use super::{
    Identifier,
    render::{TaskMutationAction, render_mutation},
};
use crate::console::Console;

#[derive(Args, Debug)]
pub struct Arguments {
    #[command(flatten)]
    pub(crate) identifier: Identifier,
}

pub(super) async fn run(
    arguments: &Arguments,
    console: Console,
    settings: &UserSettings,
    client: &TaskClient,
) -> anyhow::Result<String> {
    let id = arguments.identifier.id();
    let response = client
        .backlog_task(pb::BacklogTaskRequest { id: id.to_string() })
        .await
        .map_err(crate::rpc_error)?;
    let (action, task) = match response.outcome {
        Some(pb::backlog_task_response::Outcome::Backlogged(result)) => {
            (TaskMutationAction::Backlogged, result.task)
        }
        Some(pb::backlog_task_response::Outcome::AlreadyBacklogged(result)) => {
            (TaskMutationAction::AlreadyBacklogged, result.task)
        }
        None => anyhow::bail!("pwf-server returned an invalid backlog outcome"),
    };
    render_mutation(
        action,
        id.as_ref(),
        task.as_ref(),
        settings,
        console.color(),
    )
}
