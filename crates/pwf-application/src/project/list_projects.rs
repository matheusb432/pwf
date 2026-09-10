use std::error::Error;

use pwf_wire::project::ProjectStatusFilter;

use super::{Project, ProjectRowError};

#[derive(Debug, thiserror::Error)]
pub enum ListProjectsError {
    #[error("{context}: {source}")]
    Unexpected {
        context: &'static str,
        #[source]
        source: anyhow::Error,
    },
}

/// Lists managed projects in ascending title order.
#[cqrsy::query]
pub async fn execute(
    status: ProjectStatusFilter,
    pool: &sqlx::SqlitePool,
) -> Result<Vec<Project>, ListProjectsError> {
    let includes_paused = status.includes_paused();
    let rows = project_query!(
        "WHERE (? OR projects.paused_at IS NULL) ORDER BY projects.title ASC",
        includes_paused,
    )
    .fetch_all(pool)
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
