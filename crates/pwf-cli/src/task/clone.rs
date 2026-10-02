use clap::Args;
use pwf_client::{pb::CloneTaskRequest, project::ProjectClient, task::TaskClient};
use pwf_models::{project::ProjectId, settings::UserSettings};

use super::{
    Identifier,
    render::{TaskMutationAction, render_mutation},
};
use crate::console::Console;

#[derive(Args, Debug)]
pub struct Arguments {
    #[command(flatten)]
    identifier: Identifier,
    /// Destination project ID; defaults to the source task's project.
    #[arg(long, value_name = "PROJECT", value_parser = crate::project::parse_project_id)]
    project: Option<ProjectId>,
}

pub(super) async fn run(
    arguments: &Arguments,
    console: Console,
    settings: &UserSettings,
    client: &TaskClient,
    projects: &ProjectClient,
) -> Result<String, crate::error::Error> {
    let id = arguments.identifier.id();
    let project_id = match arguments.project.as_ref() {
        Some(id) => Some(
            crate::project::resolve_project_id(id, projects)
                .await?
                .into_inner(),
        ),
        None => None,
    };
    let result = client
        .clone_task(CloneTaskRequest {
            id: id.to_string(),
            project_id,
        })
        .await?;
    render_mutation(
        TaskMutationAction::Cloned,
        &result.id,
        result.task.as_ref(),
        settings,
        console.color(),
    )
    .map_err(Into::into)
}
