use std::{path::PathBuf, str::FromStr};

use clap::{Args, ValueEnum};
use pwf_application::project::add_project::{self, AddProject};
use pwf_models::project::{ProjectSource, ProjectSourceKind, ProjectTasks, ProjectTasksKind};
use pwf_wire::project::ProjectFields;
use serde::Deserialize;
use sqlx::SqlitePool;

use super::{
    output, parse_project_id, parse_project_source, parse_project_tasks, parse_project_title,
};

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
pub struct DirectoryPayload(pub ProjectFields);

impl FromStr for DirectoryPayload {
    type Err = String;

    fn from_str(payload: &str) -> Result<Self, Self::Err> {
        let payload: AddPayload = serde_json::from_str(payload)
            .map_err(|error| format!("project add JSON is invalid: {error}"))?;
        let id = parse_project_id(&payload.id)?;
        let title = parse_project_title(&payload.title)?;
        let source_value = parse_project_source(&payload.source.value)?;
        let TasksPayload::Directory { path } = payload.tasks;
        let tasks_path = parse_project_tasks(&path)?;

        Ok(Self(ProjectFields {
            id,
            title,
            source: ProjectSource::new(ProjectSourceKind::Directory, source_value),
            tasks: ProjectTasks::new(ProjectTasksKind::Directory, tasks_path),
        }))
    }
}

pub(super) async fn run(
    arguments: Arguments,
    pool: &SqlitePool,
    home: PathBuf,
) -> Result<String, String> {
    match arguments.kind {
        SourceKind::Directory => {
            let project = add_project::execute(
                AddProject {
                    fields: arguments.payload.0,
                    home,
                },
                pool,
            )
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
