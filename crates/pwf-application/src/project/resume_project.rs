use std::error::Error;

use pwf_models::project::{HomeDirectory, ProjectId, ProjectTasksPath};
use pwf_wire::project::ProjectStateChange;

use super::{ProjectRow, TaskLocationError, task_location};

#[derive(Debug, thiserror::Error)]
pub enum ResumeProjectError {
    #[error("project not found: {id}")]
    ProjectNotFound { id: ProjectId },
    #[error(transparent)]
    TaskLocation(#[from] TaskLocationError),
    #[error("{context}: {source}")]
    Unexpected {
        context: &'static str,
        #[source]
        source: anyhow::Error,
    },
}

/// Resumes one project and reports whether persisted state changed.
#[cqrsy::command]
pub async fn execute(
    project_id: ProjectId,
    pool: &sqlx::SqlitePool,
    home: &HomeDirectory,
) -> Result<ProjectStateChange, ResumeProjectError> {
    let mut transaction = pool
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
    let update = sqlx::query!(
        r#"
        UPDATE projects
        SET paused_at = NULL
        WHERE id = ? AND paused_at IS NOT NULL
        "#,
        id,
    )
    .execute(&mut *transaction)
    .await
    .map_err(|error| unexpected("resuming project", error))?;
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
    .map_err(|error| unexpected("reading resumed project", error))?
    .ok_or(ResumeProjectError::ProjectNotFound {
        id: project_id.clone(),
    })?;
    let project = super::project_from_row(row)
        .map_err(|error| unexpected("converting resumed project", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| unexpected("committing project resume", error))?;

    Ok(ProjectStateChange {
        project,
        changed: update.rows_affected() == 1,
    })
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use pwf_models::project::{HomeDirectory, ProjectId};

    use crate::{project::resume_project, testing::insert_project};

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn runtime_alias_of_other_paused_project_is_rejected(pool: sqlx::SqlitePool) {
        let home_path = PathBuf::from("/home/tester");
        let home = HomeDirectory::new(home_path.clone());
        let resolved_path = home_path.join("tasks/shared");
        insert_project(&pool, "PWF", "pwf", "/work/PWF", "~/tasks/shared", true).await;
        insert_project(
            &pool,
            "ALT",
            "alt",
            "/work/ALT",
            &resolved_path.to_string_lossy(),
            true,
        )
        .await;

        let error = resume_project::execute(ProjectId::try_new("PWF").unwrap(), &pool, &home)
            .await
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            format!(
                "managed projects ALT and PWF resolve to the same task location: {}",
                resolved_path.display()
            )
        );
        let paused_at: Option<String> =
            sqlx::query_scalar("SELECT paused_at FROM projects WHERE id = 'PWF'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(paused_at.is_some());
        pool.close().await;
    }
}
