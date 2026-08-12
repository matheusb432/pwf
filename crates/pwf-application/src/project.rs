//! Provides managed-project application interactors.

use pwf_models::project::{Project, ProjectCreatedAt, ProjectId, ProjectName};

#[derive(Debug)]
pub(in crate::project) struct ProjectRow {
    pub(in crate::project) id: String,
    pub(in crate::project) title: String,
    pub(in crate::project) source_kind: String,
    pub(in crate::project) source_value: String,
    pub(in crate::project) tasks_kind: String,
    pub(in crate::project) tasks_path: String,
    pub(in crate::project) created_at: String,
    pub(in crate::project) is_paused: bool,
}

#[derive(Debug, thiserror::Error)]
#[error("persisted project {field} value {value:?} is invalid: {reason}")]
pub(in crate::project) struct ProjectRowError {
    pub(in crate::project) field: &'static str,
    pub(in crate::project) value: String,
    pub(in crate::project) reason: String,
}

pub mod add_project;
pub mod get_active_project;
pub mod get_project;
pub mod list_projects;
pub mod pause_project;
pub mod rename_project;
pub mod resolve_project;
pub mod resume_project;
pub mod runtime_path;
mod task_location;

fn project_from_row(row: ProjectRow) -> Result<Project, ProjectRowError> {
    let id = project_value("id", row.id, ProjectId::try_new)?;
    let title = project_value("title", row.title, ProjectName::try_new)?;
    let source_kind = project_value("source kind", row.source_kind, |value| {
        pwf_models::project::ProjectSourceKind::try_from(value.as_str())
    })?;
    let source_value = project_value(
        "source value",
        row.source_value,
        pwf_models::project::ProjectSourceValue::try_new,
    )?;
    let tasks_kind = project_value("tasks kind", row.tasks_kind, |value| {
        pwf_models::project::ProjectTasksKind::try_from(value.as_str())
    })?;
    let tasks_path = project_value(
        "tasks path",
        row.tasks_path,
        pwf_models::project::ProjectTasksPath::try_new,
    )?;
    let created_at = project_value("created_at", row.created_at, ProjectCreatedAt::try_new)?;

    Ok(Project {
        id,
        title,
        source: pwf_models::project::ProjectSource::new(source_kind, source_value),
        tasks: pwf_models::project::ProjectTasks::new(tasks_kind, tasks_path),
        created_at,
        is_paused: row.is_paused,
    })
}

fn project_value<T, E>(
    field: &'static str,
    value: String,
    conversion: impl FnOnce(String) -> Result<T, E>,
) -> Result<T, ProjectRowError>
where
    E: std::fmt::Display,
{
    conversion(value.clone()).map_err(|error| ProjectRowError {
        field,
        value,
        reason: error.to_string(),
    })
}
