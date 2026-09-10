use clap::Args;
use pwf_client::{pb::CloneTaskRequest, project::ProjectClient, task::TaskClient};
use pwf_models::{project::ProjectSelector, settings::TaskStatusColors};

use super::{
    Identifier,
    render::{TaskMutationAction, render_mutation},
};
use crate::console::Console;

#[derive(Args, Debug)]
pub struct Arguments {
    #[command(flatten)]
    identifier: Identifier,
    /// Destination project name or ID; defaults to the source task's project.
    #[arg(long, value_name = "PROJECT")]
    project: Option<ProjectSelector>,
}

pub(super) async fn run(
    arguments: &Arguments,
    console: Console,
    colors: TaskStatusColors,
    client: &TaskClient,
    projects: &ProjectClient,
) -> anyhow::Result<String> {
    let id = arguments.identifier.id();
    let project_id = match arguments.project.as_ref() {
        Some(selector) => Some(
            crate::project::resolve_project_id(selector, projects)
                .await?
                .into_inner(),
        ),
        None => None,
    };
    let result = client
        .clone_task(CloneTaskRequest {
            id: id.to_string(),
            project_id,
            request_id: String::new(),
        })
        .await
        .map_err(crate::rpc_error)?;
    render_mutation(
        TaskMutationAction::Cloned,
        &result.id,
        result.task.as_ref(),
        colors,
        console.color(),
    )
}
