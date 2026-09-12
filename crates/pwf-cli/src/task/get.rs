use clap::Args;
use pwf_client::{
    pb::{GetProjectRequest, GetTaskRecordRequest, GetTaskRequest, ProjectStatusFilter},
    project::ProjectClient,
    task::TaskClient,
};
use pwf_models::settings::UserSettings;

use super::{ContentFormat, ContentSelection, Identifier, render::content};
use crate::console::Console;

#[derive(Args, Debug)]
pub struct Arguments {
    #[command(flatten)]
    pub(crate) identifier: Identifier,
    /// Print the task's file path.
    #[arg(long)]
    pub(crate) path: bool,
    /// Full content format. Defaults to rich on terminals and md when redirected.
    #[arg(long, num_args = 0..=1, require_equals = true, default_missing_value = "", value_parser = ContentSelection::parse, value_name = "rich|md|json", conflicts_with = "path")]
    pub(crate) long: Option<ContentSelection>,
}

pub(super) async fn run(
    arguments: &Arguments,
    console: Console,
    settings: &UserSettings,
    client: &TaskClient,
    projects: &ProjectClient,
) -> anyhow::Result<String> {
    let id = arguments.identifier.id();
    let format = arguments
        .long
        .unwrap_or(ContentSelection::Automatic)
        .resolve(console);
    if format == ContentFormat::Json {
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
        return content::json(&task, project.title);
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
    if format == ContentFormat::Rich {
        content::rich(
            &content::TaskContent::from(&record),
            settings,
            console.color(),
            console.stdout_columns(),
        )
    } else {
        Ok(record.source)
    }
}
