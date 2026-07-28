use pwf_application::project::{Project, ProjectStateChange};
use pwf_domain::project::{ProjectSourceKind, ProjectTasksKind};
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

pub(super) fn project(project: Project) -> Result<String, String> {
    render(&ProjectOutput::from(project))
}

pub(super) fn projects(projects: Vec<Project>) -> Result<String, String> {
    render(
        &projects
            .into_iter()
            .map(ProjectOutput::from)
            .collect::<Vec<_>>(),
    )
}

pub(super) fn state_change(change: ProjectStateChange) -> Result<String, String> {
    render(&ProjectStateChangeOutput {
        project: ProjectOutput::from(change.project),
        changed: change.changed,
    })
}

fn render(value: &impl Serialize) -> Result<String, String> {
    serde_json::to_string_pretty(value)
        .map_err(|error| format!("rendering project JSON failed: {error}"))
}

impl From<Project> for ProjectOutput {
    fn from(project: Project) -> Self {
        Self {
            id: project.id.to_string(),
            title: project.title.to_string(),
            source: ProjectSourceOutput {
                kind: match project.source.kind() {
                    ProjectSourceKind::Directory => ProjectSourceKindOutput::Directory,
                },
                value: project.source.value().to_string(),
            },
            tasks: ProjectTasksOutput {
                kind: match project.tasks.kind() {
                    ProjectTasksKind::Directory => ProjectTasksKindOutput::Directory,
                },
                path: project.tasks.path().to_string(),
            },
            created_at: project.created_at,
            is_paused: project.is_paused,
        }
    }
}
