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
