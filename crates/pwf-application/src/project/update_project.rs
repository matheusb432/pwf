use std::error::Error;

use pwf_models::project::ProjectId;
use pwf_wire::project::UpdateProject;

use super::source_record;

#[derive(Debug, thiserror::Error)]
pub enum UpdateProjectError {
    #[error("project not found: {id}")]
    ProjectNotFound { id: ProjectId },
    #[error("{context}: {source}")]
    Unexpected {
        context: &'static str,
        #[source]
        source: anyhow::Error,
    },
}

/// Updates one managed project's source.
#[cqrsy::command]
pub async fn execute(
    command: UpdateProject,
    pool: &sqlx::SqlitePool,
) -> Result<(), UpdateProjectError> {
    let mut transaction = pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|error| unexpected("starting project update transaction", error))?;
    let source_id = source_record::get_or_insert(&mut transaction, &command.source)
        .await
        .map_err(|error| unexpected("resolving project source", error))?;
    let project_id = command.id.as_ref();
    let update = sqlx::query!(
        "UPDATE projects SET project_source_id = ? WHERE id = ?",
        source_id,
        project_id,
    )
    .execute(&mut *transaction)
    .await
    .map_err(|error| unexpected("updating project", error))?;
    match update.rows_affected() {
        1 => {}
        0 => {
            transaction
                .rollback()
                .await
                .map_err(|error| unexpected("rolling back missing project update", error))?;
            return Err(UpdateProjectError::ProjectNotFound { id: command.id });
        }
        count => {
            return Err(unexpected(
                "updating project",
                std::io::Error::other(format!(
                    "project update changed {count} rows; expected exactly one"
                )),
            ));
        }
    }
    transaction
        .commit()
        .await
        .map_err(|error| unexpected("committing project update", error))
}

fn unexpected(
    context: &'static str,
    source: impl Error + Send + Sync + 'static,
) -> UpdateProjectError {
    UpdateProjectError::Unexpected {
        context,
        source: anyhow::Error::new(source),
    }
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use pwf_models::project::{ProjectSource, ProjectSourceKind, ProjectSourceValue};
    use pwf_wire::project::UpdateProject;

    use super::UpdateProjectError;
    use crate::{project::update_project, testing::insert_project};

    fn update(project_id: &str, source_value: &str) -> UpdateProject {
        UpdateProject {
            id: project_id.parse().unwrap(),
            source: ProjectSource::new(
                ProjectSourceKind::Directory,
                ProjectSourceValue::try_new(source_value).unwrap(),
            ),
        }
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn source_update_is_atomic_idempotent_and_reuses_shared_sources(pool: sqlx::SqlitePool) {
        insert_project(&pool, "FOO", "foo", "/work/old", "/tasks/foo", true).await;
        insert_project(&pool, "BAR", "bar", "/work/shared", "/tasks/bar", false).await;

        update_project::execute(update("FOO", "/work/shared"), &pool)
            .await
            .unwrap();
        update_project::execute(update("FOO", "/work/shared"), &pool)
            .await
            .unwrap();

        let stored: (String, String, String, String, bool) = sqlx::query_as(
            r"
            SELECT
                projects.id,
                projects.title,
                project_sources.value,
                projects.tasks_path,
                projects.paused_at IS NOT NULL
            FROM projects
            JOIN project_sources ON project_sources.id = projects.project_source_id
            WHERE projects.id = 'FOO'
            ",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            stored,
            (
                "FOO".to_string(),
                "foo".to_string(),
                "/work/shared".to_string(),
                "/tasks/foo".to_string(),
                true,
            )
        );
        let shared_source_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM project_sources WHERE value = '/work/shared'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(shared_source_count, 1);
        pool.close().await;
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn missing_project_rolls_back_the_candidate_source(pool: sqlx::SqlitePool) {
        let error = update_project::execute(update("MISS", "/work/candidate"), &pool)
            .await
            .unwrap_err();

        assert_matches!(
            error,
            UpdateProjectError::ProjectNotFound { id } if id.as_ref() == "MISS"
        );
        let source_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM project_sources WHERE value = '/work/candidate'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(source_count, 0);
        pool.close().await;
    }
}
