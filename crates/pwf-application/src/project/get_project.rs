use std::error::Error;

use pwf_models::project::ProjectId;

use super::{
    Project, ProjectStatusFilter,
    dto::{ProjectRow, ProjectRowError},
};

/// Requests one managed project by project ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetProject {
    /// Project ID.
    pub id: ProjectId,
    /// Project statuses eligible for the lookup.
    pub status: ProjectStatusFilter,
}

#[derive(Debug, thiserror::Error)]
pub enum GetProjectError {
    #[error("project not found: {id}")]
    ProjectNotFound { id: ProjectId },
    #[error("{context}: {source}")]
    Unexpected {
        context: &'static str,
        #[source]
        source: Box<dyn Error + Send + Sync>,
    },
}

/// Reads one managed project.
///
/// # Errors
///
/// Returns [`GetProjectError::ProjectNotFound`] when no project has the requested ID. Returns
/// [`GetProjectError::Unexpected`] for database and persisted-data failures.
#[cqrsy::query]
pub async fn execute(
    query: GetProject,
    pool: &sqlx::SqlitePool,
) -> Result<Project, GetProjectError> {
    let id = query.id.as_ref();
    let includes_paused = query.status.includes_paused();
    let row = sqlx::query_as!(
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
        WHERE projects.id = ?
          AND (? OR projects.paused_at IS NULL)
        "#,
        id,
        includes_paused,
    )
    .fetch_optional(pool)
    .await
    .map_err(|error| unexpected("reading project", error))?
    .ok_or(GetProjectError::ProjectNotFound { id: query.id })?;

    super::logic::project_from_row(row).map_err(|error| unexpected_row("converting project", error))
}

fn unexpected(
    context: &'static str,
    source: impl Error + Send + Sync + 'static,
) -> GetProjectError {
    GetProjectError::Unexpected {
        context,
        source: Box::new(source),
    }
}

fn unexpected_row(context: &'static str, source: ProjectRowError) -> GetProjectError {
    unexpected(context, source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::insert_project;

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn status_filter_controls_paused_project_visibility(pool: sqlx::SqlitePool) {
        insert_project(
            &pool,
            "PWF".parse().unwrap(),
            "pwf",
            "/work/pwf",
            "/tasks/pwf",
            true,
        )
        .await;
        let id = ProjectId::try_new("PWF").unwrap();

        let active = super::execute(
            GetProject {
                id: id.clone(),
                status: ProjectStatusFilter::ACTIVE,
            },
            &pool,
        )
        .await;
        let all = super::execute(
            GetProject {
                id,
                status: ProjectStatusFilter::ALL,
            },
            &pool,
        )
        .await
        .unwrap();

        assert!(matches!(
            active,
            Err(GetProjectError::ProjectNotFound { .. })
        ));
        assert!(all.is_paused);
    }
}
