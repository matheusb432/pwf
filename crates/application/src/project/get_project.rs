use std::error::Error;

use pwf_domain::project::ProjectPrefix;

use super::{
    Project,
    dto::{ProjectRow, ProjectRowError},
};
use crate::AppDbStore;

/// Requests one managed project by canonical prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetProject {
    /// Project prefix.
    pub id: ProjectPrefix,
}

/// Reports a missing project or unexpected project read failure.
#[derive(Debug, thiserror::Error)]
pub enum GetProjectError {
    /// The requested project does not exist.
    #[error("project not found: {id}")]
    ProjectNotFound {
        /// Missing project prefix.
        id: ProjectPrefix,
    },
    /// Project retrieval failed outside expected absence.
    #[error("{context}: {source}")]
    Unexpected {
        /// Failed operation boundary.
        context: &'static str,
        /// Concrete database or persisted-data failure.
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
    database: &impl AppDbStore,
) -> Result<Project, GetProjectError> {
    let id = query.id.as_ref();
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
        "#,
        id,
    )
    .fetch_optional(database.pool())
    .await
    .map_err(|error| unexpected("reading project", error))?
    .ok_or(GetProjectError::ProjectNotFound { id: query.id })?;

    Project::try_from(row).map_err(|error| unexpected_row("converting project", error))
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
