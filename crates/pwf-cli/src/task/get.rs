use clap::Args;
use pwf_client::{
    pb::{GetProjectRequest, GetTaskRecordRequest, GetTaskRequest, ProjectStatusFilter},
    project::ProjectClient,
    task::TaskClient,
};

use super::Identifier;

mod output;

#[derive(Args, Debug)]
pub struct Arguments {
    #[command(flatten)]
    pub(crate) identifier: Identifier,
    /// Print the task's note path instead of the note markdown.
    #[arg(long)]
    pub(crate) path: bool,
    /// Print typed task data as JSON.
    #[arg(long, conflicts_with = "path")]
    pub(crate) json: bool,
}

pub(super) async fn run(
    arguments: &Arguments,
    client: &TaskClient,
    projects: &ProjectClient,
) -> anyhow::Result<String> {
    let id = arguments.identifier.id();
    if arguments.json {
        let task = client
            .get_task(GetTaskRequest { id: id.to_string() })
            .await
            .map_err(crate::rpc_error)?;
        let project = projects
            .get_project(GetProjectRequest {
                id: task.id.project_id().to_string(),
                status: ProjectStatusFilter::ActiveOnly as i32,
            })
            .await
            .map_err(crate::rpc_error)?;
        return output::json(&task, project.title);
    }
    let record = client
        .get_task_record(GetTaskRecordRequest { id: id.to_string() })
        .await
        .map_err(crate::rpc_error)?
        .record
        .ok_or_else(|| anyhow::anyhow!("pwf-server returned an empty task record"))?;
    if arguments.path {
        return Ok(record.locator);
    }
    Ok(record.source)
}
