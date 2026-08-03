use pwf_models::{
    project::{Project, ProjectId},
    task::ProjectName,
};

use super::dto::{ProjectRow, ProjectRowError};

pub(super) fn project_from_row(row: ProjectRow) -> Result<Project, ProjectRowError> {
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
    if row.created_at.is_empty() {
        return Err(ProjectRowError {
            field: "created_at",
            value: row.created_at,
            reason: "value cannot be empty".to_string(),
        });
    }

    Ok(Project {
        id,
        title,
        source: pwf_models::project::ProjectSource::new(source_kind, source_value),
        tasks: pwf_models::project::ProjectTasks::new(tasks_kind, tasks_path),
        created_at: row.created_at,
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

pub(in crate::project) mod task_location {
    use std::path::{Path, PathBuf};

    use pwf_models::project::ProjectId;

    use crate::project::resolve_runtime_path::{
        self, ResolveRuntimePath, ResolvedPath, RuntimePathError,
    };

    #[derive(Debug)]
    pub(in crate::project) enum TaskLocationError {
        InvalidPath {
            project_id: ProjectId,
            path: String,
            source: RuntimePathError,
        },
        Collision {
            first_id: ProjectId,
            second_id: ProjectId,
            path: PathBuf,
        },
    }

    pub(in crate::project) fn reject_collision(
        candidate_id: &ProjectId,
        candidate_path: &str,
        existing: impl IntoIterator<Item = (ProjectId, String)>,
        home: &Path,
    ) -> Result<ResolvedPath, TaskLocationError> {
        let candidate = resolve_path(candidate_id, candidate_path, home)?;
        let mut existing = existing.into_iter().collect::<Vec<_>>();
        existing.sort_unstable_by(|(left_id, _), (right_id, _)| left_id.cmp(right_id));

        for (existing_id, existing_path) in existing {
            let existing = resolve_path(&existing_id, &existing_path, home)?;
            if candidate.identity() == existing.identity() {
                let (first_id, second_id) = if candidate_id <= &existing_id {
                    (candidate_id.clone(), existing_id)
                } else {
                    (existing_id, candidate_id.clone())
                };
                return Err(TaskLocationError::Collision {
                    first_id,
                    second_id,
                    path: candidate.path().to_path_buf(),
                });
            }
        }

        Ok(candidate)
    }

    fn resolve_path(
        project_id: &ProjectId,
        path: &str,
        home: &Path,
    ) -> Result<ResolvedPath, TaskLocationError> {
        resolve_runtime_path::execute(&ResolveRuntimePath {
            path: path.to_string(),
            home: home.to_path_buf(),
        })
        .map_err(|source| TaskLocationError::InvalidPath {
            project_id: project_id.clone(),
            path: path.to_string(),
            source,
        })
    }
}
