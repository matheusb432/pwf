use anyhow::Context as _;
use pwf_client::v1::{Project, ProjectStateChange};
use serde::Serialize;

#[derive(Serialize)]
struct ProjectOutput {
    id: String,
    title: String,
    source: ProjectSourceOutput,
    tasks: ProjectTasksOutput,
    created_at: String,
    is_paused: bool,
}

#[derive(Serialize)]
struct ProjectSourceOutput {
    kind: ProjectSourceKindOutput,
    value: String,
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum ProjectSourceKindOutput {
    Directory,
}

#[derive(Serialize)]
struct ProjectTasksOutput {
    kind: ProjectTasksKindOutput,
    path: String,
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum ProjectTasksKindOutput {
    Directory,
}

#[derive(Serialize)]
struct ProjectStateChangeOutput {
    project: ProjectOutput,
    changed: bool,
}

pub(super) fn project(project: Project) -> anyhow::Result<String> {
    render(&ProjectOutput::from(project)).map_err(Into::into)
}

pub(super) fn projects(projects: Vec<Project>) -> anyhow::Result<String> {
    render(
        &projects
            .into_iter()
            .map(ProjectOutput::from)
            .collect::<Vec<_>>(),
    )
    .map_err(Into::into)
}

pub(super) fn state_change(change: ProjectStateChange) -> anyhow::Result<String> {
    let project = change
        .project
        .context("pwf-server returned a project state change without a project")?;
    render(&ProjectStateChangeOutput {
        project: ProjectOutput::from(project),
        changed: change.changed,
    })
    .map_err(Into::into)
}

fn render(value: &impl Serialize) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(value)
}

impl From<Project> for ProjectOutput {
    fn from(project: Project) -> Self {
        Self {
            id: project.id,
            title: project.title,
            source: ProjectSourceOutput {
                kind: ProjectSourceKindOutput::Directory,
                value: project.source_value,
            },
            tasks: ProjectTasksOutput {
                kind: ProjectTasksKindOutput::Directory,
                path: project.tasks_path,
            },
            created_at: project.created_at,
            is_paused: project.is_paused,
        }
    }
}
