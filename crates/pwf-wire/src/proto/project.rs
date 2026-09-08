//! Explicit protobuf mappings for project operations.

use pwf_models::project::{
    Project, ProjectId, ProjectName, ProjectSource, ProjectSourceKind, ProjectSourceValue,
    ProjectTasks, ProjectTasksKind, ProjectTasksPath,
};
use tonic::Status;

use super::{invalid, parse, required};
use crate::{field_update::FieldUpdate, pb, project};

pub fn add_project_request(
    request: pb::AddProjectRequest,
) -> Result<project::ProjectFields, Status> {
    project_fields(required("fields", request.fields)?)
}

pub fn add_vault_project_request(
    request: pb::AddVaultProjectRequest,
) -> Result<project::AddVaultProject, Status> {
    Ok(project::AddVaultProject {
        vault_path: parse("vault_path", &request.vault_path)?,
        id: parse("id", &request.id)?,
        tasks_path: parse("tasks_path", &request.tasks_path)?,
        title: request
            .title
            .map(|value| ProjectName::try_new(value).map_err(|error| invalid("title", error)))
            .transpose()?,
        source_path: request
            .source_path
            .map(|value| {
                ProjectSourceValue::try_new(value).map_err(|error| invalid("source_path", error))
            })
            .transpose()?,
    })
}

#[must_use]
pub fn add_vault_project_response(id: ProjectId) -> pb::AddVaultProjectResponse {
    pb::AddVaultProjectResponse {
        id: id.into_inner(),
    }
}

pub fn get_project_request(request: pb::GetProjectRequest) -> Result<project::GetProject, Status> {
    let pb::GetProjectRequest { id, status } = request;
    Ok(project::GetProject {
        id: parse("id", &id)?,
        status: project_status_filter(status)?,
    })
}

pub fn list_projects_request(
    request: pb::ListProjectsRequest,
) -> Result<project::ProjectStatusFilter, Status> {
    project_status_filter(request.status)
}

pub fn pause_project_request(request: pb::PauseProjectRequest) -> Result<ProjectId, Status> {
    let pb::PauseProjectRequest { id } = request;
    parse("id", &id)
}

pub fn rename_project_request(
    request: pb::RenameProjectRequest,
) -> Result<project::RenameProject, Status> {
    Ok(project::RenameProject {
        current_id: parse("current_id", &request.current_id)?,
        fields: project_fields(required("fields", request.fields)?)?,
    })
}

pub fn resume_project_request(request: pb::ResumeProjectRequest) -> Result<ProjectId, Status> {
    let pb::ResumeProjectRequest { id } = request;
    parse("id", &id)
}

pub fn update_project_request(
    request: pb::UpdateProjectRequest,
) -> Result<project::UpdateProject, Status> {
    let source_value = match source_value_update(request.source_value)? {
        FieldUpdate::Update(source_value) => FieldUpdate::Update(ProjectSource::new(
            ProjectSourceKind::Directory,
            source_value,
        )),
        FieldUpdate::Clear => FieldUpdate::Clear,
        FieldUpdate::Unchanged => FieldUpdate::Unchanged,
    };
    let obsidian_vault = match request.obsidian_vault {
        None => FieldUpdate::Unchanged,
        Some(update) => match required("obsidian_vault.operation", update.operation)? {
            pb::string_field_update::Operation::Clear(_) => FieldUpdate::Clear,
            pb::string_field_update::Operation::Update(value) => FieldUpdate::Update(
                pwf_models::project::ObsidianVault::try_new(value)
                    .map_err(|error| invalid("obsidian_vault", error))?,
            ),
        },
    };
    if source_value.is_unchanged() && matches!(obsidian_vault, FieldUpdate::Unchanged) {
        return Err(invalid("project", "at least one update is required"));
    }
    Ok(project::UpdateProject {
        id: parse("id", &request.id)?,
        source: source_value,
        obsidian_vault,
    })
}

#[must_use]
pub fn add_project_response(project: Project) -> pb::AddProjectResponse {
    let Project {
        id,
        title,
        source,
        tasks,
        created_at,
        is_paused,
        obsidian_vault,
    } = project;
    pb::AddProjectResponse {
        id: id.to_string(),
        title: title.to_string(),
        source_kind: source.as_ref().map(|source| source.kind().to_string()),
        source_value: source.map(|source| source.value().to_string()),
        tasks_kind: tasks.kind().to_string(),
        tasks_path: tasks.path().to_string(),
        created_at: created_at.to_string(),
        is_paused,
        obsidian_vault: obsidian_vault.map(|value| value.to_string()),
    }
}

#[must_use]
pub fn get_project_response(project: Project) -> pb::GetProjectResponse {
    let Project {
        id,
        title,
        source,
        tasks,
        created_at,
        is_paused,
        obsidian_vault,
    } = project;
    pb::GetProjectResponse {
        id: id.to_string(),
        title: title.to_string(),
        source_kind: source.as_ref().map(|source| source.kind().to_string()),
        source_value: source.map(|source| source.value().to_string()),
        tasks_kind: tasks.kind().to_string(),
        tasks_path: tasks.path().to_string(),
        created_at: created_at.to_string(),
        is_paused,
        obsidian_vault: obsidian_vault.map(|value| value.to_string()),
    }
}

pub fn list_projects_response(projects: Vec<Project>) -> pb::ListProjectsResponse {
    pb::ListProjectsResponse {
        projects: projects.into_iter().map(project_message).collect(),
    }
}

#[must_use]
pub fn pause_project_response(change: project::ProjectStateChange) -> pb::PauseProjectResponse {
    pb::PauseProjectResponse {
        project: Some(project_message(change.project)),
        changed: change.changed,
    }
}

#[must_use]
pub fn rename_project_response(project: Project) -> pb::RenameProjectResponse {
    let Project {
        id,
        title,
        source,
        tasks,
        created_at,
        is_paused,
        obsidian_vault,
    } = project;
    pb::RenameProjectResponse {
        id: id.to_string(),
        title: title.to_string(),
        source_kind: source.as_ref().map(|source| source.kind().to_string()),
        source_value: source.map(|source| source.value().to_string()),
        tasks_kind: tasks.kind().to_string(),
        tasks_path: tasks.path().to_string(),
        created_at: created_at.to_string(),
        is_paused,
        obsidian_vault: obsidian_vault.map(|value| value.to_string()),
    }
}

#[must_use]
pub fn resume_project_response(change: project::ProjectStateChange) -> pb::ResumeProjectResponse {
    pb::ResumeProjectResponse {
        project: Some(project_message(change.project)),
        changed: change.changed,
    }
}

#[must_use]
pub fn update_project_response() -> pb::UpdateProjectResponse {
    pb::UpdateProjectResponse {}
}

fn project_fields(fields: pb::ProjectFields) -> Result<project::ProjectFields, Status> {
    let source = match (fields.source_kind, fields.source_value) {
        (None, None) => None,
        (Some(kind), Some(value)) => Some(ProjectSource::new(
            ProjectSourceKind::try_from(kind.as_str())
                .map_err(|error| invalid("fields.source_kind", error))?,
            ProjectSourceValue::try_new(value)
                .map_err(|error| invalid("fields.source_value", error))?,
        )),
        _ => {
            return Err(invalid(
                "fields.source",
                "kind and value must both be present or absent",
            ));
        }
    };
    let tasks_kind = ProjectTasksKind::try_from(fields.tasks_kind.as_str())
        .map_err(|error| invalid("fields.tasks_kind", error))?;
    Ok(project::ProjectFields {
        id: ProjectId::try_new(fields.id)
            .map_err(|_| invalid("fields.id", "expected two to four ASCII letters"))?,
        title: ProjectName::try_new(fields.title)
            .map_err(|error| invalid("fields.title", error))?,
        source,
        obsidian_vault: fields
            .obsidian_vault
            .map(|value| {
                pwf_models::project::ObsidianVault::try_new(value)
                    .map_err(|error| invalid("fields.obsidian_vault", error))
            })
            .transpose()?,
        tasks: ProjectTasks::new(
            tasks_kind,
            ProjectTasksPath::try_new(fields.tasks_path)
                .map_err(|_| invalid("fields.tasks_path", "must not be blank"))?,
        ),
    })
}

fn source_value_update(
    update: Option<pb::StringFieldUpdate>,
) -> Result<FieldUpdate<ProjectSourceValue>, Status> {
    let Some(update) = update else {
        return Ok(FieldUpdate::Unchanged);
    };
    match required("source_value.operation", update.operation)? {
        pb::string_field_update::Operation::Update(value) => ProjectSourceValue::try_new(value)
            .map(FieldUpdate::Update)
            .map_err(|_| invalid("source_value", "must not be blank")),
        pb::string_field_update::Operation::Clear(_) => Ok(FieldUpdate::Clear),
    }
}

fn project_message(project: Project) -> pb::Project {
    let Project {
        id,
        title,
        source,
        tasks,
        created_at,
        is_paused,
        obsidian_vault,
    } = project;
    pb::Project {
        id: id.to_string(),
        title: title.to_string(),
        source_kind: source.as_ref().map(|source| source.kind().to_string()),
        source_value: source.map(|source| source.value().to_string()),
        tasks_kind: tasks.kind().to_string(),
        tasks_path: tasks.path().to_string(),
        created_at: created_at.to_string(),
        is_paused,
        obsidian_vault: obsidian_vault.map(|value| value.to_string()),
    }
}

fn project_status_filter(value: i32) -> Result<project::ProjectStatusFilter, Status> {
    match pb::ProjectStatusFilter::try_from(value).ok() {
        Some(pb::ProjectStatusFilter::ActiveOnly) => Ok(project::ProjectStatusFilter::ActiveOnly),
        Some(pb::ProjectStatusFilter::IncludingPaused) => {
            Ok(project::ProjectStatusFilter::IncludingPaused)
        }
        Some(pb::ProjectStatusFilter::Unspecified) | None => {
            Err(invalid("status", "must be specified"))
        }
    }
}

#[cfg(test)]
mod tests {
    use tonic::Code;

    use crate::pb::{StringFieldUpdate, UpdateProjectRequest, string_field_update};

    #[test]
    fn update_project_request_requires_one_valid_source_update() {
        let command = super::update_project_request(UpdateProjectRequest {
            obsidian_vault: None,
            id: "foo".to_string(),
            source_value: Some(StringFieldUpdate {
                operation: Some(string_field_update::Operation::Update(
                    "/work/new".to_string(),
                )),
            }),
        })
        .unwrap();

        assert_eq!(command.id.as_ref(), "FOO");
        assert!(
            matches!(command.source, crate::field_update::FieldUpdate::Update(value) if value.value().as_ref() == "/work/new")
        );

        for source_value in [
            None,
            Some(StringFieldUpdate { operation: None }),
            Some(StringFieldUpdate {
                operation: Some(string_field_update::Operation::Update(" ".to_string())),
            }),
        ] {
            let error = super::update_project_request(UpdateProjectRequest {
                obsidian_vault: None,
                id: "FOO".to_string(),
                source_value,
            })
            .unwrap_err();

            assert_eq!(error.code(), Code::InvalidArgument);
        }
    }
}
