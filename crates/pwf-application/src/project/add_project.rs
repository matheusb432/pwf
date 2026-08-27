use std::error::Error;

use pwf_models::project::{HomeDirectory, ProjectId, ProjectName, ProjectTasksPath};
use pwf_wire::project::ProjectFields;
use sqlx::error::ErrorKind;

use super::{Project, ProjectRow, TaskLocationError, task_location};

#[derive(Debug, thiserror::Error)]
pub enum AddProjectError {
    #[error("project id already exists: {id}")]
    DuplicateProjectId { id: ProjectId },
    #[error("project title already exists: {title}")]
    DuplicateProjectTitle { title: ProjectName },
    #[error(transparent)]
    TaskLocation(#[from] TaskLocationError),
    #[error("{context}: {source}")]
    Unexpected {
        context: &'static str,
        #[source]
        source: anyhow::Error,
    },
}

/// Creates one managed project and reuses an identical source location.
#[cqrsy::command]
pub async fn execute(
    fields: ProjectFields,
    pool: &sqlx::SqlitePool,
    home: &HomeDirectory,
) -> Result<Project, AddProjectError> {
    let mut transaction = pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|error| unexpected("starting project creation transaction", error))?;
    let existing = other_task_locations(&mut transaction, &fields.id).await?;
    task_location::reject_collision(&fields.id, fields.tasks.path(), existing, home)?;
    let source_kind = fields.source.kind().to_string();
    let source_value = fields.source.value().as_ref();
    let source_id = sqlx::query_scalar!(
        r#"
        SELECT id AS "id!"
        FROM project_sources
        WHERE kind = ? AND value = ?
        "#,
        source_kind,
        source_value,
    )
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| unexpected("reading project source", error))?;
    let source_id = match source_id {
        Some(source_id) => source_id,
        None => sqlx::query!(
            r#"
            INSERT INTO project_sources (kind, value)
            VALUES (?, ?)
            "#,
            source_kind,
            source_value,
        )
        .execute(&mut *transaction)
        .await
        .map_err(|error| unexpected("inserting project source", error))?
        .last_insert_rowid(),
    };

    let id = fields.id.as_ref();
    let title = fields.title.as_ref();
    let tasks_kind = fields.tasks.kind().to_string();
    let tasks_path = fields.tasks.path().as_ref();
    let insert_result = sqlx::query!(
        r#"
        INSERT INTO projects (id, project_source_id, title, tasks_kind, tasks_path)
        VALUES (?, ?, ?, ?, ?)
        "#,
        id,
        source_id,
        title,
        tasks_kind,
        tasks_path,
    )
    .execute(&mut *transaction)
    .await;
    if let Err(error) = insert_result {
        return Err(project_insertion_error(&mut transaction, &fields, error).await);
    }

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
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| unexpected("reading created project", error))?;
    let project = super::project_from_row(row)
        .map_err(|error| unexpected("converting created project", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| unexpected("committing project creation", error))?;
    Ok(project)
}

async fn other_task_locations(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    candidate_id: &ProjectId,
) -> Result<Vec<(ProjectId, ProjectTasksPath)>, AddProjectError> {
    sqlx::query_as::<_, (String, String)>(
        "SELECT id, tasks_path FROM projects WHERE id != ? ORDER BY id ASC",
    )
    .bind(candidate_id.as_ref())
    .fetch_all(&mut **transaction)
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
    .collect()
}

async fn project_insertion_error(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    fields: &ProjectFields,
    error: sqlx::Error,
) -> AddProjectError {
    if !error
        .as_database_error()
        .is_some_and(|database_error| database_error.kind() == ErrorKind::UniqueViolation)
    {
        return unexpected("inserting project", error);
    }

    let id = fields.id.as_ref();
    let title = fields.title.as_ref();
    let conflicts = sqlx::query!(
        r#"
        SELECT
            EXISTS(SELECT 1 FROM projects WHERE id = ?) AS "id_exists!: bool",
            EXISTS(SELECT 1 FROM projects WHERE title = ?) AS "title_exists!: bool"
        "#,
        id,
        title,
    )
    .fetch_one(&mut **transaction)
    .await;
    let conflicts = match conflicts {
        Ok(conflicts) => conflicts,
        Err(query_error) => return unexpected("classifying project conflict", query_error),
    };
    if conflicts.id_exists {
        return AddProjectError::DuplicateProjectId {
            id: fields.id.clone(),
        };
    }
    if conflicts.title_exists {
        return AddProjectError::DuplicateProjectTitle {
            title: fields.title.clone(),
        };
    }
    unexpected("inserting project", error)
}

fn unexpected(
    context: &'static str,
    source: impl Error + Send + Sync + 'static,
) -> AddProjectError {
    AddProjectError::Unexpected {
        context,
        source: anyhow::Error::new(source),
    }
}

#[cfg(test)]
mod tests {
    use pwf_models::project::{
        HomeDirectory, ProjectId, ProjectName, ProjectSource, ProjectSourceKind,
        ProjectSourceValue, ProjectTasks, ProjectTasksKind, ProjectTasksPath,
    };
    use pwf_wire::project::ProjectFields;

    use super::*;
    use crate::{project::add_project, testing::insert_project};

    fn project(
        project_id: &str,
        title: &str,
        source_value: &str,
        tasks_path: &str,
    ) -> ProjectFields {
        ProjectFields {
            id: project_id.parse().expect("valid test project ID"),
            title: ProjectName::try_new(title).unwrap(),
            source: ProjectSource::new(
                ProjectSourceKind::Directory,
                ProjectSourceValue::try_new(source_value).unwrap(),
            ),
            tasks: ProjectTasks::new(
                ProjectTasksKind::Directory,
                ProjectTasksPath::try_new(tasks_path).unwrap(),
            ),
        }
    }

    fn home() -> HomeDirectory {
        HomeDirectory::new("/home/tester".into())
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn matching_source_row_is_reused(pool: sqlx::SqlitePool) {
        let home = home();

        add_project::execute(
            project("ONE", "one", "/work/shared", "/tasks/one"),
            &pool,
            &home,
        )
        .await
        .unwrap();
        add_project::execute(
            project("TWO", "two", "/work/shared", "/tasks/two"),
            &pool,
            &home,
        )
        .await
        .unwrap();

        let source_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM project_sources")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(source_count, 1);
        pool.close().await;
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn duplicate_project_id_is_classified(pool: sqlx::SqlitePool) {
        let home = home();
        add_project::execute(
            project("PWF", "pwf", "/work/pwf", "/tasks/pwf"),
            &pool,
            &home,
        )
        .await
        .unwrap();

        let error = add_project::execute(
            project("pwf", "other", "/work/other", "/tasks/other"),
            &pool,
            &home,
        )
        .await
        .unwrap_err();

        assert!(matches!(
            error,
            AddProjectError::DuplicateProjectId { id }
                if id == ProjectId::try_new("PWF").unwrap()
        ));
        pool.close().await;
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn duplicate_project_title_is_classified(pool: sqlx::SqlitePool) {
        let home = home();
        add_project::execute(
            project("PWF", "pwf", "/work/pwf", "/tasks/pwf"),
            &pool,
            &home,
        )
        .await
        .unwrap();

        let error = add_project::execute(
            project("ALT", "pwf", "/work/other", "/tasks/other"),
            &pool,
            &home,
        )
        .await
        .unwrap_err();

        assert!(matches!(
            error,
            AddProjectError::DuplicateProjectTitle { title }
                if title == ProjectName::try_new("pwf").unwrap()
        ));
        pool.close().await;
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn failed_project_insert_rolls_back_new_source(pool: sqlx::SqlitePool) {
        let home = home();
        add_project::execute(
            project("PWF", "pwf", "/work/pwf", "/tasks/pwf"),
            &pool,
            &home,
        )
        .await
        .unwrap();

        let error = add_project::execute(
            project("ALT", "pwf", "/work/rolled-back", "/tasks/other"),
            &pool,
            &home,
        )
        .await
        .unwrap_err();

        assert!(matches!(
            error,
            AddProjectError::DuplicateProjectTitle { .. }
        ));
        let source_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM project_sources WHERE value = ?")
                .bind("/work/rolled-back")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(source_count, 0);
        pool.close().await;
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn runtime_alias_of_paused_project_is_rejected(pool: sqlx::SqlitePool) {
        let home_path = std::path::PathBuf::from("/home/tester");
        let home = HomeDirectory::new(home_path.clone());
        let resolved_path = home_path.join("tasks/shared");
        insert_project(&pool, "PWF", "pwf", "/work/PWF", "~/tasks/shared", true).await;

        let error = add_project::execute(
            project(
                "ALT",
                "other",
                "/work/ALT",
                &resolved_path.to_string_lossy(),
            ),
            &pool,
            &home,
        )
        .await
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            format!(
                "managed projects ALT and PWF resolve to the same task location: {}",
                resolved_path.display()
            )
        );
        pool.close().await;
    }
}
