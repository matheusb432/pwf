use std::path::PathBuf;

use clap::Args;
use pwf_application::project::{
    ProjectFields,
    rename_project::{self, RenameProject},
};
use pwf_infra::obsidian::ObsidianProjectTaskFilesClient;
use pwf_models::project::{
    ProjectId, ProjectName, ProjectSource, ProjectSourceKind, ProjectSourceValue, ProjectTasks,
    ProjectTasksKind, ProjectTasksPath,
};
use sqlx::SqlitePool;

use super::{output, parse_project_id};

#[derive(Args, Debug)]
pub struct Arguments {
    /// Existing canonical project ID.
    #[arg(value_parser = parse_project_id)]
    pub current_id: ProjectId,
    /// Replacement canonical project ID.
    #[arg(value_parser = parse_project_id)]
    pub destination_id: ProjectId,
    /// Replacement project title.
    #[arg(long, value_parser = parse_project_title)]
    pub title: ProjectName,
    /// Replacement project source directory.
    #[arg(long, value_parser = parse_project_source)]
    pub source: ProjectSourceValue,
    /// Replacement pending-work task directory.
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

fn parse_project_title(raw: &str) -> Result<ProjectName, String> {
    ProjectName::try_new(raw.to_string()).map_err(|_| {
        if raw.trim().eq_ignore_ascii_case("project") {
            "project title is reserved".to_string()
        } else {
            "project title must not be blank".to_string()
        }
    })
}

fn parse_project_source(raw: &str) -> Result<ProjectSourceValue, String> {
    ProjectSourceValue::try_new(raw.to_string())
        .map_err(|_| "project source value must not be blank".to_string())
}

fn parse_project_tasks(raw: &str) -> Result<ProjectTasksPath, String> {
    ProjectTasksPath::try_new(raw.to_string())
        .map_err(|_| "project tasks path must not be blank".to_string())
}
