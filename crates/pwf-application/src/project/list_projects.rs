use std::error::Error;

use pwf_wire::project::ProjectStatusFilter;

use super::{Project, ProjectRow, ProjectRowError};

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
    let rows = if status.includes_paused() {
        sqlx::query_as!(
            ProjectRow,
            r#"
            SELECT
                projects.id AS "id!",
                projects.title AS "title!",
                project_sources.kind AS "source_kind?",
                project_sources.value AS "source_value?",
                projects.tasks_kind AS "tasks_kind!",
                projects.tasks_path AS "tasks_path!",
                projects.obsidian_vault AS "obsidian_vault?",
                projects.created_at AS "created_at!",
                (projects.paused_at IS NOT NULL) AS "is_paused!: bool"
            FROM projects
            LEFT JOIN project_sources ON project_sources.id = projects.project_source_id
            ORDER BY projects.title ASC
            "#,
        )
        .fetch_all(pool)
        .await
        .map_err(|error| unexpected("listing projects", error))?
    } else {
        sqlx::query_as!(
            ProjectRow,
            r#"
            SELECT
                active_projects.id AS "id!",
                active_projects.title AS "title!",
                project_sources.kind AS "source_kind?",
                project_sources.value AS "source_value?",
                active_projects.tasks_kind AS "tasks_kind!",
                active_projects.tasks_path AS "tasks_path!",
                active_projects.obsidian_vault AS "obsidian_vault?",
                active_projects.created_at AS "created_at!",
                false AS "is_paused!: bool"
            FROM active_projects
            LEFT JOIN project_sources ON project_sources.id = active_projects.project_source_id
            ORDER BY active_projects.title ASC
            "#,
        )
        .fetch_all(pool)
        .await
        .map_err(|error| unexpected("listing active projects", error))?
    };

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
