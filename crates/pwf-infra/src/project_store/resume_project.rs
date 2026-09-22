use std::error::Error;

use pwf_application::project::resume_project::ResumeProjectError;
use pwf_models::project::{HomeDirectory, ProjectId, ProjectTasksPath};
use pwf_wire::project::ProjectStateChange;
use sqlx::Connection as _;

use super::task_location;
pub(super) async fn execute(
    project_id: ProjectId,
    connection: &mut sqlx::SqliteConnection,
    home: &HomeDirectory,
) -> Result<ProjectStateChange, ResumeProjectError> {
    let mut transaction = connection
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|error| unexpected("starting project resume transaction", error))?;
    let id = project_id.as_ref();
    let candidate_path =
        sqlx::query_scalar::<_, String>("SELECT tasks_path FROM projects WHERE id = ?")
            .bind(id)
            .fetch_optional(&mut *transaction)
            .await
            .map_err(|error| unexpected("reading resumed project task location", error))?
            .ok_or_else(|| ResumeProjectError::ProjectNotFound {
                id: project_id.clone(),
            })?;
    let candidate_path = ProjectTasksPath::try_new(candidate_path)
        .map_err(|error| unexpected("converting resumed project task location", error))?;
    let existing = sqlx::query_as::<_, (String, String)>(
        "SELECT id, tasks_path FROM projects WHERE id != ? ORDER BY id ASC",
    )
    .bind(id)
    .fetch_all(&mut *transaction)
    .await
    .map_err(|error| unexpected("reading project task locations", error))?
    .into_iter()
    .map(|(id, tasks_path)| {
        let id = ProjectId::try_new(id)
            .map_err(|error| unexpected("converting project task location id", error))?;
        let tasks_path = ProjectTasksPath::try_new(tasks_path)
            .map_err(|error| unexpected("converting project task location path", error))?;
        Ok((id, tasks_path))
    })
    .collect::<Result<Vec<_>, ResumeProjectError>>()?;
    task_location::reject_collision(&project_id, &candidate_path, existing, home)?;
    let mut rows = project_returning!(
        r#"
        UPDATE projects
        SET paused_at = NULL
        WHERE id = ? AND paused_at IS NOT NULL
        "#,
        id,
    )
    .fetch_all(&mut *transaction)
    .await
    .map_err(|error| unexpected("resuming project", error))?;
    let (row, changed) = match rows.len() {
        0 => {
            let row = project_query!("WHERE projects.id = ?", id)
                .fetch_optional(&mut *transaction)
                .await
                .map_err(|error| unexpected("reading resumed project", error))?
                .ok_or(ResumeProjectError::ProjectNotFound {
                    id: project_id.clone(),
                })?;
            (row, false)
        }
        1 => {
            let row = rows.pop().ok_or_else(|| {
                unexpected(
                    "resuming project",
                    std::io::Error::other("project resume returned no row"),
                )
            })?;
            (row, true)
        }
        count => {
            return Err(unexpected(
                "resuming project",
                std::io::Error::other(format!(
                    "project resume returned {count} rows; expected at most one"
                )),
            ));
        }
    };
    let project = super::project_from_row(row)
        .map_err(|error| unexpected("converting resumed project", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| unexpected("committing project resume", error))?;

    Ok(ProjectStateChange { project, changed })
}

fn unexpected(
    context: &'static str,
    source: impl Error + Send + Sync + 'static,
) -> ResumeProjectError {
    ResumeProjectError::Unexpected {
        context,
        source: anyhow::Error::new(source),
    }
}
