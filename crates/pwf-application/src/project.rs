//! Provides managed-project application interactors.

use pwf_models::project::{Project, ProjectCreatedAt, ProjectId, ProjectName};

#[derive(Debug)]
pub(in crate::project) struct ProjectRow {
    pub(in crate::project) id: String,
    pub(in crate::project) title: String,
    pub(in crate::project) source_kind: Option<String>,
    pub(in crate::project) source_value: Option<String>,
    pub(in crate::project) tasks_kind: String,
    pub(in crate::project) tasks_path: String,
    pub(in crate::project) obsidian_vault: Option<String>,
    pub(in crate::project) created_at: String,
    pub(in crate::project) is_paused: bool,
}

#[derive(Debug, thiserror::Error)]
#[error("persisted project {field} value {value:?} is invalid: {source}")]
pub(in crate::project) struct ProjectRowError {
    pub(in crate::project) field: &'static str,
    pub(in crate::project) value: String,
    #[source]
    pub(in crate::project) source: anyhow::Error,
}

// SQLx checks the complete literal query after this macro expands.
macro_rules! project_query {
    ($suffix:literal $(, $argument:expr)* $(,)?) => {
        ::sqlx::query_as!(
            $crate::project::ProjectRow,
            r#"
            SELECT
                projects.id AS "id!",
                projects.title AS "title!",
                project_sources.kind AS "source_kind?",
                project_sources.value AS "source_value?",
                projects.tasks_kind AS "tasks_kind!",
                projects.tasks_path AS "tasks_path!",
                projects.obsidian_vault AS "obsidian_vault?",
                projects.created_at AS "created_at!",
                (projects.paused_at IS NOT NULL) AS "is_paused!: bool"
            FROM projects
            LEFT JOIN project_sources ON project_sources.id = projects.project_source_id
            "# + $suffix,
            $($argument),*
        )
    };
}

pub mod add_project;
pub mod add_vault_project;
pub mod get_active_project;
pub mod get_project;
pub mod get_projects;
pub mod list_projects;
pub mod pause_project;
pub mod rename_project;
pub mod resolve_project;
pub mod resume_project;
pub mod runtime_path;
mod source_record;
mod task_location;
pub mod update_project;

pub use task_location::TaskLocationError;

fn project_from_row(row: ProjectRow) -> Result<Project, ProjectRowError> {
    let id = project_value("id", row.id, ProjectId::try_new)?;
    let title = project_value("title", row.title, ProjectName::try_new)?;
    let source = match (row.source_kind, row.source_value) {
        (None, None) => None,
        (Some(kind), Some(value)) => Some(pwf_models::project::ProjectSource::new(
            project_value("source kind", kind, |value| {
                pwf_models::project::ProjectSourceKind::try_from(value.as_str())
            })?,
            project_value(
                "source value",
                value,
                pwf_models::project::ProjectSourceValue::try_new,
            )?,
        )),
        (kind, value) => {
            return Err(ProjectRowError {
                field: "source",
                value: format!("{kind:?}: {value:?}"),
                source: anyhow::anyhow!("source kind and value must both be present or absent"),
            });
        }
    };
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
        source,
        tasks: pwf_models::project::ProjectTasks::new(tasks_kind, tasks_path),
        obsidian_vault: row
            .obsidian_vault
            .map(|value| {
                project_value(
                    "obsidian_vault",
                    value,
                    pwf_models::project::ObsidianVault::try_new,
                )
            })
            .transpose()?,
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
    E: std::error::Error + Send + Sync + 'static,
{
    conversion(value.clone()).map_err(|source| ProjectRowError {
        field,
        value,
        source: anyhow::Error::new(source),
    })
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use super::{ProjectRow, project_from_row};

    #[test]
    fn invalid_persisted_project_values_retain_the_model_error() {
        let error = project_from_row(ProjectRow {
            obsidian_vault: None,
            id: "FOO".to_string(),
            title: "x".repeat(201),
            source_kind: Some("directory".to_string()),
            source_value: Some("/work/foo".to_string()),
            tasks_kind: "directory".to_string(),
            tasks_path: "/tasks/foo".to_string(),
            created_at: "2026-07-25T00:00:00.000Z".to_string(),
            is_paused: false,
        })
        .unwrap_err();

        assert_eq!(error.field, "title");
        assert!(error.source().is_some());
    }
}
