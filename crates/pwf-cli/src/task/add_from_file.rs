use std::path::PathBuf;

use anyhow::Context as _;
use clap::Args;
use pwf_client::{pb::CreateTaskFromFileRequest, project::ProjectClient, task::TaskClient};
use pwf_models::{project::ProjectId, settings::TaskStatusColors};

use super::render::{TaskMutationAction, render_mutation};
use crate::console::Console;

#[derive(Args, Debug)]
pub struct Arguments {
    /// UTF-8 Markdown file to use as the task body
    #[arg(value_name = "FILE")]
    source_file: PathBuf,
    /// Managed project ID (two to four ASCII letters)
    #[arg(long, value_name = "PROJECT", value_parser = crate::project::parse_project_id)]
    project: ProjectId,
}

pub(super) async fn run(
    arguments: &Arguments,
    console: Console,
    colors: TaskStatusColors,
    client: &TaskClient,
    projects: &ProjectClient,
) -> Result<String, crate::error::Error> {
    let project_id = crate::project::resolve_project_id(&arguments.project, projects).await?;
    let source_file = std::path::absolute(&arguments.source_file)
        .context("resolving task source file path")?
        .into_os_string()
        .into_string()
        .map_err(|_| anyhow::anyhow!("task source file path is not valid unicode"))?;
    let added = client
        .create_task_from_file(CreateTaskFromFileRequest {
            project_id: project_id.to_string(),
            source_file,
            title: None,
        })
        .await?;
    render_mutation(
        TaskMutationAction::Added,
        &added.id,
        added.task.as_ref(),
        colors,
        console.color(),
    )
    .map_err(Into::into)
}
