use std::error::Error;

use pwf_application::project::get_project::GetProjectError;
use pwf_models::project::Project;
use pwf_wire::project::GetProject;
pub(super) async fn execute(
    query: GetProject,
    connection: &mut sqlx::SqliteConnection,
) -> Result<Project, GetProjectError> {
    let id = query.id.as_ref();
    let includes_paused = query.status.includes_paused();
    let row = project_query!(
        "WHERE projects.id = ? AND (? OR projects.paused_at IS NULL)",
        id,
        includes_paused,
    )
    .fetch_optional(&mut *connection)
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
