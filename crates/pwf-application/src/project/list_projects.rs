use std::error::Error;

use pwf_wire::project::ProjectStatusFilter;

use super::{Project, ProjectRow, ProjectRowError};

/// Requests managed projects in ascending title order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListProjects {
    /// Project statuses eligible for the list.
    pub status: ProjectStatusFilter,
}

#[derive(Debug, thiserror::Error)]
pub enum ListProjectsError {
    #[error("{context}: {source}")]
    Unexpected {
        context: &'static str,
        #[source]
        source: Box<dyn Error + Send + Sync>,
    },
}

/// Lists managed projects in ascending title order.
///
/// # Errors
///
/// Returns [`ListProjectsError`] for database and persisted-data failures.
#[cqrsy::query]
pub async fn execute(
    query: ListProjects,
    pool: &sqlx::SqlitePool,
) -> Result<Vec<Project>, ListProjectsError> {
    let rows = if query.status.includes_paused() {
        sqlx::query_as!(
            ProjectRow,
            r#"
            SELECT
                projects.id AS "id!",
                projects.title AS "title!",
                project_sources.kind AS "source_kind!",
                project_sources.value AS "source_value!",
                projects.tasks_kind AS "tasks_kind!",
                projects.tasks_path AS "tasks_path!",
                projects.created_at AS "created_at!",
                (projects.paused_at IS NOT NULL) AS "is_paused!: bool"
            FROM projects
            JOIN project_sources ON project_sources.id = projects.project_source_id
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
                project_sources.kind AS "source_kind!",
                project_sources.value AS "source_value!",
                active_projects.tasks_kind AS "tasks_kind!",
                active_projects.tasks_path AS "tasks_path!",
                active_projects.created_at AS "created_at!",
                false AS "is_paused!: bool"
            FROM active_projects
            JOIN project_sources ON project_sources.id = active_projects.project_source_id
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
        .map_err(|error| unexpected_row("converting listed project", error))
}

fn unexpected(
    context: &'static str,
    source: impl Error + Send + Sync + 'static,
) -> ListProjectsError {
    ListProjectsError::Unexpected {
        context,
        source: Box::new(source),
    }
}

fn unexpected_row(context: &'static str, source: ProjectRowError) -> ListProjectsError {
    unexpected(context, source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::insert_project;

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn active_list_filters_paused_projects_and_sorts_by_title(pool: sqlx::SqlitePool) {
        insert_project(&pool, "ZED", "zeta", "/work/zeta", "/tasks/zeta", false).await;
        insert_project(&pool, "ALP", "alpha", "/work/alpha", "/tasks/alpha", false).await;
        insert_project(&pool, "PAU", "beta", "/work/beta", "/tasks/beta", true).await;

        let projects = super::execute(
            ListProjects {
                status: ProjectStatusFilter::ACTIVE,
            },
            &pool,
        )
        .await
        .unwrap();

        assert_eq!(
            projects
                .iter()
                .map(|project| project.title.as_ref())
                .collect::<Vec<_>>(),
            ["alpha", "zeta"]
        );
        assert!(projects.iter().all(|project| !project.is_paused));
        pool.close().await;
    }
}
