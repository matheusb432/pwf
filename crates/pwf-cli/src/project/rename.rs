use std::path::PathBuf;

use clap::Args;
use pwf_application::project::rename_project::{self, RenameProject};
use pwf_infra::obsidian::ObsidianProjectTaskFilesClient;
use pwf_models::project::{
    ProjectId, ProjectName, ProjectSource, ProjectSourceKind, ProjectSourceValue, ProjectTasks,
    ProjectTasksKind, ProjectTasksPath,
};
use pwf_wire::project::ProjectFields;
use sqlx::SqlitePool;

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

pub(super) async fn run(
    arguments: Arguments,
    pool: &SqlitePool,
    home: PathBuf,
) -> Result<String, String> {
    let fields = ProjectFields {
        id: arguments.destination_id,
        title: arguments.title,
        source: ProjectSource::new(ProjectSourceKind::Directory, arguments.source),
        tasks: ProjectTasks::new(ProjectTasksKind::Directory, arguments.tasks),
    };
    let renamed = rename_project::execute(
        RenameProject {
            current_id: arguments.current_id,
            fields,
            home,
        },
        pool,
        &ObsidianProjectTaskFilesClient,
    )
    .await
    .map_err(|error| error.to_string())?;
    output::project(renamed)
}
