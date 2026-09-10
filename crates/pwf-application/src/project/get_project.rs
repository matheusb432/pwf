use std::error::Error;

use pwf_models::project::ProjectId;
use pwf_wire::project::GetProject;

use super::Project;

#[derive(Debug, thiserror::Error)]
pub enum GetProjectError {
    #[error("project not found: {id}")]
    ProjectNotFound { id: ProjectId },
    #[error("{context}: {source}")]
    Unexpected {
        context: &'static str,
        #[source]
        source: anyhow::Error,
    },
}

/// Reads one managed project.
#[cqrsy::query]
pub async fn execute(
    query: GetProject,
    pool: &sqlx::SqlitePool,
) -> Result<Project, GetProjectError> {
    let id = query.id.as_ref();
    let includes_paused = query.status.includes_paused();
    let row = project_query!(
        "WHERE projects.id = ? AND (? OR projects.paused_at IS NULL)",
        id,
        includes_paused,
    )
    .fetch_optional(pool)
    .await
    .map_err(|error| unexpected("reading project", error))?
    .ok_or(GetProjectError::ProjectNotFound { id: query.id })?;

    super::project_from_row(row).map_err(|error| unexpected("converting project", error))
}

fn unexpected(
    context: &'static str,
    source: impl Error + Send + Sync + 'static,
) -> GetProjectError {
    GetProjectError::Unexpected {
        context,
        source: anyhow::Error::new(source),
    }
}
