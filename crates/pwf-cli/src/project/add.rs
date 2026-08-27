use std::str::FromStr;

use clap::{Args, ValueEnum};
use pwf_client::{
    project::ProjectClient,
    v1::{AddProjectRequest, ProjectFields},
};
use serde::Deserialize;

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
            id: id.to_string(),
            title: title.to_string(),
            source_kind: "directory".to_string(),
            source_value: source_value.to_string(),
            tasks_kind: "directory".to_string(),
            tasks_path: tasks_path.to_string(),
        }))
    }
}

pub(super) async fn run(arguments: Arguments, client: &ProjectClient) -> anyhow::Result<String> {
    match arguments.kind {
        SourceKind::Directory => {
            let project = client
                .add_project(AddProjectRequest {
                    fields: Some(arguments.payload.0),
                })
                .await
                .map_err(crate::rpc_error)?;
            output::render_response(project)
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
