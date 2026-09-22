use std::error::Error;

use pwf_application::project::pause_project::PauseProjectError;
use pwf_models::project::ProjectId;
use pwf_wire::project::ProjectStateChange;
use sqlx::Connection as _;
pub(super) async fn execute(
    project_id: ProjectId,
    connection: &mut sqlx::SqliteConnection,
) -> Result<ProjectStateChange, PauseProjectError> {
    let mut transaction = connection
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|error| unexpected("starting project pause transaction", error))?;
    let id = project_id.as_ref();
    let mut rows = project_returning!(
        r#"
        UPDATE projects
        SET paused_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
        WHERE id = ? AND paused_at IS NULL
        "#,
        id,
    )
    .fetch_all(&mut *transaction)
    .await
    .map_err(|error| unexpected("pausing project", error))?;
    let (row, changed) = match rows.len() {
        0 => {
            let row = project_query!("WHERE projects.id = ?", id)
                .fetch_optional(&mut *transaction)
                .await
                .map_err(|error| unexpected("reading paused project", error))?
                .ok_or(PauseProjectError::ProjectNotFound { id: project_id })?;
            (row, false)
        }
        1 => {
            let row = rows.pop().ok_or_else(|| {
                unexpected(
                    "pausing project",
                    std::io::Error::other("project pause returned no row"),
                )
            })?;
            (row, true)
        }
        count => {
            return Err(unexpected(
                "pausing project",
                std::io::Error::other(format!(
                    "project pause returned {count} rows; expected at most one"
                )),
            ));
        }
    };
    let project = super::project_from_row(row)
        .map_err(|error| unexpected("converting paused project", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| unexpected("committing project pause", error))?;

    Ok(ProjectStateChange { project, changed })
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
