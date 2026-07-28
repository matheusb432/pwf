use std::error::Error;

use super::{
    Project,
    dto::{ProjectRow, ProjectRowError},
};
use crate::AppDbStore;

/// Requests managed projects in ascending title order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListProjects {
    /// Includes paused projects when true.
    pub include_paused: bool,
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
    database: &impl AppDbStore,
) -> Result<Vec<Project>, ListProjectsError> {
    let rows = if query.include_paused {
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
        .fetch_all(database.pool())
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
        .fetch_all(database.pool())
        .await
        .map_err(|error| unexpected("listing active projects", error))?
    };

    rows.into_iter()
        .map(Project::try_from)
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
    use crate::ports::TestDatabase;

    #[tokio::test]
    async fn active_list_filters_paused_projects_and_sorts_by_title() {
        let database = TestDatabase::new().await;
        database
            .insert_project("ZED", "zeta", "/work/zeta", "/tasks/zeta", false)
            .await;
        database
            .insert_project("ALP", "alpha", "/work/alpha", "/tasks/alpha", false)
            .await;
        database
            .insert_project("PAU", "beta", "/work/beta", "/tasks/beta", true)
            .await;

        let projects = super::execute(
            ListProjects {
                include_paused: false,
            },
            &database,
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
    }
}
