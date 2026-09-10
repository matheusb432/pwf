use anyhow::Context as _;
use pwf_client::pb::{
    AddProjectResponse, GetProjectResponse, PauseProjectResponse, Project, RenameProjectResponse,
    ResumeProjectResponse,
};
use serde::Serialize;

#[derive(Serialize)]
pub(super) struct ProjectOutput {
    id: String,
    title: String,
    source: Option<ProjectSourceOutput>,
    tasks: ProjectTasksOutput,
    created_at: String,
    is_paused: bool,
    obsidian_vault: Option<String>,
    snapshot_enabled: bool,
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

pub(super) fn render_response(response: impl Into<ProjectOutput>) -> anyhow::Result<String> {
    render(&response.into()).map_err(Into::into)
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

pub(super) fn render_pause_project(response: PauseProjectResponse) -> anyhow::Result<String> {
    state_change(response.project, response.changed)
}

pub(super) fn render_resume_project(response: ResumeProjectResponse) -> anyhow::Result<String> {
    state_change(response.project, response.changed)
}

fn state_change(project: Option<Project>, changed: bool) -> anyhow::Result<String> {
    let project =
        project.context("pwf-server returned a project state change without a project")?;
    render(&ProjectStateChangeOutput {
        project: ProjectOutput::from(project),
        changed,
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
            source: project.source_value.map(|value| ProjectSourceOutput {
                kind: ProjectSourceKindOutput::Directory,
                value,
            }),
            tasks: ProjectTasksOutput {
                kind: ProjectTasksKindOutput::Directory,
                path: project.tasks_path,
            },
            created_at: project.created_at,
            is_paused: project.is_paused,
            obsidian_vault: project.obsidian_vault,
            snapshot_enabled: project.snapshot_enabled,
        }
    }
}

impl From<AddProjectResponse> for ProjectOutput {
    fn from(response: AddProjectResponse) -> Self {
        Self {
            id: response.id,
            title: response.title,
            source: response.source_value.map(|value| ProjectSourceOutput {
                kind: ProjectSourceKindOutput::Directory,
                value,
            }),
            tasks: ProjectTasksOutput {
                kind: ProjectTasksKindOutput::Directory,
                path: response.tasks_path,
            },
            created_at: response.created_at,
            is_paused: response.is_paused,
            obsidian_vault: response.obsidian_vault,
            snapshot_enabled: response.snapshot_enabled,
        }
    }
}

impl From<GetProjectResponse> for ProjectOutput {
    fn from(response: GetProjectResponse) -> Self {
        Self {
            id: response.id,
            title: response.title,
            source: response.source_value.map(|value| ProjectSourceOutput {
                kind: ProjectSourceKindOutput::Directory,
                value,
            }),
            tasks: ProjectTasksOutput {
                kind: ProjectTasksKindOutput::Directory,
                path: response.tasks_path,
            },
            created_at: response.created_at,
            is_paused: response.is_paused,
            obsidian_vault: response.obsidian_vault,
            snapshot_enabled: response.snapshot_enabled,
        }
    }
}

impl From<RenameProjectResponse> for ProjectOutput {
    fn from(response: RenameProjectResponse) -> Self {
        Self {
            id: response.id,
            title: response.title,
            source: response.source_value.map(|value| ProjectSourceOutput {
                kind: ProjectSourceKindOutput::Directory,
                value,
            }),
            tasks: ProjectTasksOutput {
                kind: ProjectTasksKindOutput::Directory,
                path: response.tasks_path,
            },
            created_at: response.created_at,
            is_paused: response.is_paused,
            obsidian_vault: response.obsidian_vault,
            snapshot_enabled: response.snapshot_enabled,
        }
    }
}
