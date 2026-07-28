use std::error::Error;

use pwf_domain::project::ProjectPrefix;

use super::{
    Project, ProjectStateChange,
    dto::{ProjectRow, ProjectRowError},
};
use crate::AppDbStore;

/// Requests pausing one managed project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PauseProject {
    /// Project prefix.
    pub id: ProjectPrefix,
}

#[derive(Debug, thiserror::Error)]
pub enum PauseProjectError {
    #[error("project not found: {id}")]
    ProjectNotFound { id: ProjectPrefix },
    #[error("{context}: {source}")]
    Unexpected {
        context: &'static str,
        #[source]
        source: Box<dyn Error + Send + Sync>,
    },
}

/// Pauses one project and reports whether persisted state changed.
///
/// # Errors
///
/// Returns [`PauseProjectError::ProjectNotFound`] when no project has the requested ID. Returns
/// [`PauseProjectError::Unexpected`] for database and persisted-data failures.
#[cqrsy::command]
pub async fn execute(
    command: PauseProject,
    database: &impl AppDbStore,
) -> Result<ProjectStateChange, PauseProjectError> {
    let mut transaction = database
        .pool()
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|error| unexpected("starting project pause transaction", error))?;
    let id = command.id.as_ref();
    let update = sqlx::query!(
        r#"
        UPDATE projects
        SET paused_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
        WHERE id = ? AND paused_at IS NULL
        "#,
        id,
    )
    .execute(&mut *transaction)
    .await
    .map_err(|error| unexpected("pausing project", error))?;
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
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| unexpected("reading paused project", error))?
    .ok_or(PauseProjectError::ProjectNotFound { id: command.id })?;
    let project = Project::try_from(row)
        .map_err(|error| unexpected_row("converting paused project", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| unexpected("committing project pause", error))?;

    Ok(ProjectStateChange {
        project,
        changed: update.rows_affected() == 1,
    })
}

fn unexpected(
    context: &'static str,
    source: impl Error + Send + Sync + 'static,
) -> PauseProjectError {
    PauseProjectError::Unexpected {
        context,
        source: Box::new(source),
    }
}

fn unexpected_row(context: &'static str, source: ProjectRowError) -> PauseProjectError {
    unexpected(context, source)
}
