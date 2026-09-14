use std::error::Error;

use pwf_application::project::list_projects::ListProjectsError;
use pwf_models::project::Project;
use pwf_wire::project::ProjectStatusFilter;

use super::ProjectRowError;
pub(super) async fn execute(
    status: ProjectStatusFilter,
    connection: &mut sqlx::SqliteConnection,
) -> Result<Vec<Project>, ListProjectsError> {
    let includes_paused = status.includes_paused();
    let rows = project_query!(
        "WHERE (? OR projects.paused_at IS NULL) ORDER BY projects.title ASC",
        includes_paused,
    )
    .fetch_all(&mut *connection)
    .await
    .map_err(|error| unexpected("listing projects", error))?;

    rows.into_iter()
        .map(super::project_from_row)
        .collect::<Result<Vec<_>, ProjectRowError>>()
        .map_err(|error| unexpected("converting listed project", error))
}

fn unexpected(
    context: &'static str,
    source: impl Error + Send + Sync + 'static,
) -> ListProjectsError {
    ListProjectsError::Unexpected {
        context,
        source: anyhow::Error::new(source),
    }
}
