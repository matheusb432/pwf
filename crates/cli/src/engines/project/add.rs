use std::str::FromStr;

use clap::{Args, ValueEnum};
use pwf_application::project::add::AddProject;
use pwf_domain::project::{
    ProjectName, ProjectPrefix, ProjectSource, ProjectSourceKind, ProjectSourceValue, ProjectTasks,
    ProjectTasksKind, ProjectTasksPath,
};
use pwf_infra::SqliteStore;
use serde::Deserialize;

use super::output;

#[derive(Args, Debug)]
pub struct Arguments {
    /// Project source kind.
    #[arg(long, value_enum)]
    pub kind: SourceKind,
    /// One JSON object describing the project.
    #[arg(value_name = "JSON_PAYLOAD")]
    pub payload: DirectoryPayload,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum SourceKind {
    Directory,
}

#[derive(Clone, Debug)]
pub struct DirectoryPayload(pub AddProject);

impl FromStr for DirectoryPayload {
    type Err = String;

    fn from_str(payload: &str) -> Result<Self, Self::Err> {
        let payload: AddPayload = serde_json::from_str(payload)
            .map_err(|error| format!("project add JSON is invalid: {error}"))?;
        let id = ProjectPrefix::try_new(payload.id)
            .map_err(|_| "project id must contain two to four ASCII letters".to_string())?;
        let title_raw = payload.title;
        let title = ProjectName::try_new(title_raw.clone()).map_err(|_| {
            if title_raw.trim().eq_ignore_ascii_case("project") {
                "project title is reserved".to_string()
            } else {
                "project title must not be blank".to_string()
            }
        })?;
        let source_value = ProjectSourceValue::try_new(payload.source.value)
            .map_err(|_| "project source value must not be blank".to_string())?;
        let TasksPayload::Directory { path } = payload.tasks;
        let tasks_path = ProjectTasksPath::try_new(path)
            .map_err(|_| "project tasks path must not be blank".to_string())?;

        Ok(Self(AddProject {
            id,
            title,
            source: ProjectSource::new(ProjectSourceKind::Directory, source_value),
            tasks: ProjectTasks::new(ProjectTasksKind::Directory, tasks_path),
        }))
    }
}

pub(super) async fn run(arguments: Arguments, database: &SqliteStore) -> Result<String, String> {
    match arguments.kind {
        SourceKind::Directory => {
            let project = pwf_application::project::add::execute(arguments.payload.0, database)
                .await
                .map_err(|error| format!("project add failed: {error}"))?;
            output::project(project)
        }
    }
}

#[derive(Deserialize)]
struct AddPayload {
    id: String,
    title: String,
    source: SourcePayload,
    tasks: TasksPayload,
}

#[derive(Deserialize)]
struct SourcePayload {
    value: String,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum TasksPayload {
    Directory { path: String },
}
