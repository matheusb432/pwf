use std::error::Error;

use pwf_models::project::ProjectId;
use pwf_wire::project::ProjectStateChange;

#[derive(Debug, thiserror::Error)]
pub enum PauseProjectError {
    #[error("project not found: {id}")]
    ProjectNotFound { id: ProjectId },
    #[error("{context}: {source}")]
    Unexpected {
        context: &'static str,
        #[source]
        source: anyhow::Error,
    },
}

/// Pauses one project and reports whether persisted state changed.
#[cqrsy::command]
pub async fn execute(
    project_id: ProjectId,
    pool: &sqlx::SqlitePool,
) -> Result<ProjectStateChange, PauseProjectError> {
    let mut transaction = pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|error| unexpected("starting project pause transaction", error))?;
    let id = project_id.as_ref();
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
    let row = project_query!("WHERE projects.id = ?", id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|error| unexpected("reading paused project", error))?
        .ok_or(PauseProjectError::ProjectNotFound { id: project_id })?;
    let project = super::project_from_row(row)
        .map_err(|error| unexpected("converting paused project", error))?;
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
        source: anyhow::Error::new(source),
    }
}
