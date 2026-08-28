//! Explicit protobuf mappings for project operations.

use pwf_models::project::{
    Project, ProjectId, ProjectName, ProjectSource, ProjectSourceKind, ProjectSourceValue,
    ProjectTasks, ProjectTasksKind, ProjectTasksPath,
};
use tonic::Status;

use super::{invalid, parse, required};
use crate::{project, v1};

pub fn add_project_request(
    request: v1::AddProjectRequest,
) -> Result<project::ProjectFields, Status> {
    project_fields(required("fields", request.fields)?)
}

pub fn get_project_request(request: v1::GetProjectRequest) -> Result<project::GetProject, Status> {
    let v1::GetProjectRequest { id, status } = request;
    Ok(project::GetProject {
        id: parse("id", &id)?,
        status: project_status_filter(status)?,
    })
}

pub fn list_projects_request(
    request: v1::ListProjectsRequest,
) -> Result<project::ProjectStatusFilter, Status> {
    project_status_filter(request.status)
}

pub fn pause_project_request(request: v1::PauseProjectRequest) -> Result<ProjectId, Status> {
    let v1::PauseProjectRequest { id } = request;
    parse("id", &id)
}

pub fn rename_project_request(
    request: v1::RenameProjectRequest,
) -> Result<project::RenameProject, Status> {
    Ok(project::RenameProject {
        current_id: parse("current_id", &request.current_id)?,
        fields: project_fields(required("fields", request.fields)?)?,
    })
}

pub fn resume_project_request(request: v1::ResumeProjectRequest) -> Result<ProjectId, Status> {
    let v1::ResumeProjectRequest { id } = request;
    parse("id", &id)
}

#[must_use]
pub fn add_project_response(project: Project) -> v1::AddProjectResponse {
    let Project {
        id,
        title,
        source,
        tasks,
        created_at,
        is_paused,
    } = project;
    v1::AddProjectResponse {
        id: id.to_string(),
        title: title.to_string(),
        source_kind: source.kind().to_string(),
        source_value: source.value().to_string(),
        tasks_kind: tasks.kind().to_string(),
        tasks_path: tasks.path().to_string(),
        created_at: created_at.to_string(),
        is_paused,
    }
}

#[must_use]
pub fn get_project_response(project: Project) -> v1::GetProjectResponse {
    let Project {
        id,
        title,
        source,
        tasks,
        created_at,
        is_paused,
    } = project;
    v1::GetProjectResponse {
        id: id.to_string(),
        title: title.to_string(),
        source_kind: source.kind().to_string(),
        source_value: source.value().to_string(),
        tasks_kind: tasks.kind().to_string(),
        tasks_path: tasks.path().to_string(),
        created_at: created_at.to_string(),
        is_paused,
    }
}

pub fn list_projects_response(projects: Vec<Project>) -> v1::ListProjectsResponse {
    v1::ListProjectsResponse {
        projects: projects.into_iter().map(project_message).collect(),
    }
}

#[must_use]
pub fn pause_project_response(change: project::ProjectStateChange) -> v1::PauseProjectResponse {
    v1::PauseProjectResponse {
        project: Some(project_message(change.project)),
        changed: change.changed,
    }
}

#[must_use]
pub fn rename_project_response(project: Project) -> v1::RenameProjectResponse {
    let Project {
        id,
        title,
        source,
        tasks,
        created_at,
        is_paused,
    } = project;
    v1::RenameProjectResponse {
        id: id.to_string(),
        title: title.to_string(),
        source_kind: source.kind().to_string(),
        source_value: source.value().to_string(),
        tasks_kind: tasks.kind().to_string(),
        tasks_path: tasks.path().to_string(),
        created_at: created_at.to_string(),
        is_paused,
    }
}

#[must_use]
pub fn resume_project_response(change: project::ProjectStateChange) -> v1::ResumeProjectResponse {
    v1::ResumeProjectResponse {
        project: Some(project_message(change.project)),
        changed: change.changed,
    }
}

fn project_fields(fields: v1::ProjectFields) -> Result<project::ProjectFields, Status> {
    let source_kind = ProjectSourceKind::try_from(fields.source_kind.as_str())
        .map_err(|error| invalid("fields.source_kind", error))?;
    let tasks_kind = ProjectTasksKind::try_from(fields.tasks_kind.as_str())
        .map_err(|error| invalid("fields.tasks_kind", error))?;
    Ok(project::ProjectFields {
        id: ProjectId::try_new(fields.id)
            .map_err(|_| invalid("fields.id", "expected two to four ASCII letters"))?,
        title: ProjectName::try_new(fields.title)
            .map_err(|error| invalid("fields.title", error))?,
        source: ProjectSource::new(
            source_kind,
            ProjectSourceValue::try_new(fields.source_value)
                .map_err(|_| invalid("fields.source_value", "must not be blank"))?,
        ),
        tasks: ProjectTasks::new(
            tasks_kind,
            ProjectTasksPath::try_new(fields.tasks_path)
                .map_err(|_| invalid("fields.tasks_path", "must not be blank"))?,
        ),
    })
}

fn project_message(project: Project) -> v1::Project {
    let Project {
        id,
        title,
        source,
        tasks,
        created_at,
        is_paused,
    } = project;
    v1::Project {
        id: id.to_string(),
        title: title.to_string(),
        source_kind: source.kind().to_string(),
        source_value: source.value().to_string(),
        tasks_kind: tasks.kind().to_string(),
        tasks_path: tasks.path().to_string(),
        created_at: created_at.to_string(),
        is_paused,
    }
}

fn project_status_filter(value: i32) -> Result<project::ProjectStatusFilter, Status> {
    match v1::ProjectStatusFilter::try_from(value).ok() {
        Some(v1::ProjectStatusFilter::ActiveOnly) => Ok(project::ProjectStatusFilter::ActiveOnly),
        Some(v1::ProjectStatusFilter::IncludingPaused) => {
            Ok(project::ProjectStatusFilter::IncludingPaused)
        }
        Some(v1::ProjectStatusFilter::Unspecified) | None => {
            Err(invalid("status", "must be specified"))
        }
    }
}
