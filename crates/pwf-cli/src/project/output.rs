use anyhow::Context as _;
use pwf_client::v1::{
    AddProjectResponse, GetProjectResponse, PauseProjectResponse, Project, RenameProjectResponse,
    ResumeProjectResponse,
};
use serde::Serialize;

#[derive(Serialize)]
pub(super) struct ProjectOutput {
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
        Self::new(
            project.id,
            project.title,
            project.source_value,
            project.tasks_path,
            project.created_at,
            project.is_paused,
        )
    }
}

impl From<AddProjectResponse> for ProjectOutput {
    fn from(response: AddProjectResponse) -> Self {
        Self::new(
            response.id,
            response.title,
            response.source_value,
            response.tasks_path,
            response.created_at,
            response.is_paused,
        )
    }
}

impl From<GetProjectResponse> for ProjectOutput {
    fn from(response: GetProjectResponse) -> Self {
        Self::new(
            response.id,
            response.title,
            response.source_value,
            response.tasks_path,
            response.created_at,
            response.is_paused,
        )
    }
}

impl From<RenameProjectResponse> for ProjectOutput {
    fn from(response: RenameProjectResponse) -> Self {
        Self::new(
            response.id,
            response.title,
            response.source_value,
            response.tasks_path,
            response.created_at,
            response.is_paused,
        )
    }
}

impl ProjectOutput {
    fn new(
        id: String,
        title: String,
        source_value: String,
        tasks_path: String,
        created_at: String,
        is_paused: bool,
    ) -> Self {
        Self {
            id,
            title,
            source: ProjectSourceOutput {
                kind: ProjectSourceKindOutput::Directory,
                value: source_value,
            },
            tasks: ProjectTasksOutput {
                kind: ProjectTasksKindOutput::Directory,
                path: tasks_path,
            },
            created_at,
            is_paused,
        }
    }
}
