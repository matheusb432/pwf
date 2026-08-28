use std::path::PathBuf;

use pwf_models::project::{HomeDirectory, ProjectId, ProjectTasksPath};

use crate::project::runtime_path::{self, ResolvedPath, RuntimePathError};

#[derive(Debug, thiserror::Error)]
pub enum TaskLocationError {
    #[error("managed project {project_id} task path '{path}' is invalid: {source}")]
    InvalidPath {
        project_id: ProjectId,
        path: ProjectTasksPath,
        #[source]
        source: RuntimePathError,
    },
    #[error(
        "managed projects {first_id} and {second_id} resolve to the same task location: {}",
        path.display()
    )]
    Collision {
        first_id: ProjectId,
        second_id: ProjectId,
        path: PathBuf,
    },
}

pub(in crate::project) fn reject_collision(
    candidate_id: &ProjectId,
    candidate_path: &ProjectTasksPath,
    existing: impl IntoIterator<Item = (ProjectId, ProjectTasksPath)>,
    home: &HomeDirectory,
) -> Result<ResolvedPath, TaskLocationError> {
    let candidate = resolve_path(candidate_id, candidate_path, home)?;
    let mut existing = existing.into_iter().collect::<Vec<_>>();
    existing.sort_unstable_by(|(left_id, _), (right_id, _)| left_id.cmp(right_id));

    for (existing_id, existing_path) in existing {
        let existing = resolve_path(&existing_id, &existing_path, home)?;
        if let Some(collision) = collision(candidate_id, &candidate, existing_id, &existing) {
            return Err(collision);
        }
    }

    Ok(candidate)
}

fn collision(
    candidate_id: &ProjectId,
    candidate: &ResolvedPath,
    existing_id: ProjectId,
    existing: &ResolvedPath,
) -> Option<TaskLocationError> {
    if candidate.identity() != existing.identity() {
        return None;
    }
    let (first_id, second_id) = if candidate_id <= &existing_id {
        (candidate_id.clone(), existing_id)
    } else {
        (existing_id, candidate_id.clone())
    };
    Some(TaskLocationError::Collision {
        first_id,
        second_id,
        path: candidate.path().to_path_buf(),
    })
}

fn resolve_path(
    project_id: &ProjectId,
    path: &ProjectTasksPath,
    home: &HomeDirectory,
) -> Result<ResolvedPath, TaskLocationError> {
    runtime_path::resolve(path.as_ref(), home).map_err(|source| TaskLocationError::InvalidPath {
        project_id: project_id.clone(),
        path: path.clone(),
        source,
    })
}
