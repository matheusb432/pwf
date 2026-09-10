use std::{collections::BTreeSet, error::Error};

use pwf_models::project::{Project, ProjectId};

use super::get_project::GetProjectError;

/// Reads referenced projects, including paused projects, and omits missing IDs.
#[cqrsy::query]
pub async fn execute(
    ids: &BTreeSet<ProjectId>,
    pool: &sqlx::SqlitePool,
) -> Result<Vec<Project>, GetProjectError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let ids_json = serde_json::to_string(&ids.iter().map(ProjectId::as_ref).collect::<Vec<_>>())
        .map_err(|error| unexpected("encoding project IDs", error))?;
    let rows = project_query!(
        "WHERE projects.id IN (SELECT value FROM json_each(?)) ORDER BY projects.id ASC",
        ids_json,
    )
    .fetch_all(pool)
    .await
    .map_err(|error| unexpected("reading referenced projects", error))?;
    rows.into_iter()
        .map(super::project_from_row)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| unexpected("converting referenced projects", error))
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
