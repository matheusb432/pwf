use clap::Args;
use pwf_client::{
    pb::{ProjectFields, RenameProjectRequest},
    project::ProjectClient,
};
use pwf_models::project::{ProjectId, ProjectName, ProjectSourceValue, ProjectTasksPath};

use super::{
    output, parse_project_id, parse_project_source, parse_project_tasks, parse_project_title,
};

#[derive(Args, Debug)]
pub struct Arguments {
    /// Existing project ID.
    #[arg(value_parser = parse_project_id)]
    pub current_id: ProjectId,
    /// Replacement project ID.
    #[arg(value_parser = parse_project_id)]
    pub destination_id: ProjectId,
    /// Replacement project title.
    #[arg(long, value_parser = parse_project_title)]
    pub title: ProjectName,
    /// Replacement project source directory.
    #[arg(long, value_parser = parse_project_source)]
    pub source: ProjectSourceValue,
    /// Replacement task directory.
    #[arg(long, value_parser = parse_project_tasks)]
    pub tasks: ProjectTasksPath,
}

pub(super) async fn run(arguments: Arguments, client: &ProjectClient) -> anyhow::Result<String> {
    let fields = ProjectFields {
        id: arguments.destination_id.to_string(),
        title: arguments.title.to_string(),
        source_kind: "directory".to_string(),
        source_value: arguments.source.to_string(),
        tasks_kind: "directory".to_string(),
        tasks_path: arguments.tasks.to_string(),
    };
    let renamed = client
        .rename_project(RenameProjectRequest {
            current_id: arguments.current_id.to_string(),
            fields: Some(fields),
        })
        .await
        .map_err(crate::rpc_error)?;
    output::render_response(renamed)
}
