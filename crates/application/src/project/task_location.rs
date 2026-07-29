use std::path::{Path, PathBuf};

use pwf_models::project::ProjectPrefix;

use super::{
    resolve_runtime_path::{ResolvedPath, RuntimePathError},
    runtime_path,
};

#[derive(Debug)]
pub(super) enum TaskLocationError {
    InvalidPath {
        project_id: ProjectPrefix,
        path: String,
        source: RuntimePathError,
    },
    Collision {
        first_id: ProjectPrefix,
        second_id: ProjectPrefix,
        path: PathBuf,
    },
}

pub(super) fn reject_collision(
    candidate_id: &ProjectPrefix,
    candidate_path: &str,
    existing: impl IntoIterator<Item = (ProjectPrefix, String)>,
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
    project_id: &ProjectPrefix,
    path: &str,
    home: &Path,
) -> Result<ResolvedPath, TaskLocationError> {
    runtime_path::resolve(path, home).map_err(|source| TaskLocationError::InvalidPath {
        project_id: project_id.clone(),
        path: path.to_string(),
        source,
    })
}
