use std::{collections::BTreeSet, error::Error};

use pwf_application::project::get_project::GetProjectError;
use pwf_models::project::{Project, ProjectId};
pub(super) async fn execute(
    ids: &BTreeSet<ProjectId>,
    connection: &mut sqlx::SqliteConnection,
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
    .fetch_all(&mut *connection)
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
