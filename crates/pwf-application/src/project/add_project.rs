use std::{error::Error, path::PathBuf};

use pwf_models::project::{ProjectId, ProjectName};
use sqlx::error::ErrorKind;

use super::{
    Project, ProjectFields,
    dto::{ProjectRow, ProjectRowError},
    logic::task_location::{self, TaskLocationError},
};

/// Requests creation of one managed project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddProject {
    /// Project fields parsed from user input.
    pub fields: ProjectFields,
    /// Home directory used to expand home-relative task paths.
    pub home: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum AddProjectError {
    #[error("project id already exists: {id}")]
    DuplicateProjectId { id: ProjectId },
    #[error("project title already exists: {title}")]
    DuplicateProjectTitle { title: ProjectName },
    #[error("managed project {project_id} task path '{path}' is invalid: {source}")]
    InvalidTaskPath {
        project_id: ProjectId,
        path: String,
        #[source]
        source: super::resolve_runtime_path::RuntimePathError,
    },
    #[error(
        "managed projects {first_id} and {second_id} resolve to the same task location: {}",
        path.display()
    )]
    DuplicateRuntimeTaskLocation {
        first_id: ProjectId,
        second_id: ProjectId,
        path: PathBuf,
    },
    #[error("{context}: {source}")]
    Unexpected {
        context: &'static str,
        #[source]
        source: Box<dyn Error + Send + Sync>,
    },
}

/// Creates one managed project and reuses an identical source location.
///
/// # Errors
///
/// Returns a conflict variant when the ID or title exists. Returns
/// [`AddProjectError::InvalidTaskPath`] when the candidate or an existing task path cannot be
/// resolved, and [`AddProjectError::DuplicateRuntimeTaskLocation`] when two projects resolve to
/// the same task location. Returns [`AddProjectError::Unexpected`] for database and persisted-data
/// failures.
#[cqrsy::command]
pub async fn execute(
    command: AddProject,
    pool: &sqlx::SqlitePool,
) -> Result<Project, AddProjectError> {
    let mut transaction = pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|error| unexpected("starting project creation transaction", error))?;
    let existing = other_task_locations(&mut transaction, &command.fields.id).await?;
    task_location::reject_collision(
        &command.fields.id,
        command.fields.tasks.path().as_ref(),
        existing,
        &command.home,
    )
    .map_err(task_location_error)?;
    let source_kind = command.fields.source.kind().to_string();
    let source_value = command.fields.source.value().as_ref();
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

    let id = command.fields.id.as_ref();
    let title = command.fields.title.as_ref();
    let tasks_kind = command.fields.tasks.kind().to_string();
    let tasks_path = command.fields.tasks.path().as_ref();
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
        return Err(project_insertion_error(&mut transaction, &command, error).await);
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
    let project = super::logic::project_from_row(row)
        .map_err(|error| unexpected_row("converting created project", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| unexpected("committing project creation", error))?;
    Ok(project)
}

async fn other_task_locations(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    candidate_id: &ProjectId,
) -> Result<Vec<(ProjectId, String)>, AddProjectError> {
    sqlx::query_as::<_, (String, String)>(
        "SELECT id, tasks_path FROM projects WHERE id != ? ORDER BY id ASC",
    )
    .bind(candidate_id.as_ref())
    .fetch_all(&mut **transaction)
    .await
    .map_err(|error| unexpected("reading project task locations", error))?
    .into_iter()
    .map(|(id, tasks_path)| {
        ProjectId::try_new(id)
            .map(|id| (id, tasks_path))
            .map_err(|error| unexpected("converting project task location", error))
    })
    .collect()
}

async fn project_insertion_error(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    command: &AddProject,
    error: sqlx::Error,
) -> AddProjectError {
    if !error
        .as_database_error()
        .is_some_and(|database_error| database_error.kind() == ErrorKind::UniqueViolation)
    {
        return unexpected("inserting project", error);
    }

    let id = command.fields.id.as_ref();
    let title = command.fields.title.as_ref();
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
            id: command.fields.id.clone(),
        };
    }
    if conflicts.title_exists {
        return AddProjectError::DuplicateProjectTitle {
            title: command.fields.title.clone(),
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
        source: Box::new(source),
    }
}

fn unexpected_row(context: &'static str, source: ProjectRowError) -> AddProjectError {
    unexpected(context, source)
}

fn task_location_error(error: TaskLocationError) -> AddProjectError {
    match error {
        TaskLocationError::InvalidPath {
            project_id,
            path,
            source,
        } => AddProjectError::InvalidTaskPath {
            project_id,
            path,
            source,
        },
        TaskLocationError::Collision {
            first_id,
            second_id,
            path,
        } => AddProjectError::DuplicateRuntimeTaskLocation {
            first_id,
            second_id,
            path,
        },
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use pwf_models::project::{
        ProjectId, ProjectName, ProjectSource, ProjectSourceKind, ProjectSourceValue, ProjectTasks,
        ProjectTasksKind, ProjectTasksPath,
    };

    use super::*;
    use crate::testing::insert_project;

    fn project(
        project_id: ProjectId,
        title: &str,
        source_value: &str,
        tasks_path: &str,
        home: PathBuf,
    ) -> AddProject {
        AddProject {
            fields: ProjectFields {
                id: project_id,
                title: ProjectName::try_new(title).unwrap(),
                source: ProjectSource::new(
                    ProjectSourceKind::Directory,
                    ProjectSourceValue::try_new(source_value).unwrap(),
                ),
                tasks: ProjectTasks::new(
                    ProjectTasksKind::Directory,
                    ProjectTasksPath::try_new(tasks_path).unwrap(),
                ),
            },
            home,
        }
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn matching_source_row_is_reused(pool: sqlx::SqlitePool) {
        let home = PathBuf::from("/home/tester");

        super::execute(
            project(
                "ONE".parse().unwrap(),
                "one",
                "/work/shared",
                "/tasks/one",
                home.clone(),
            ),
            &pool,
        )
        .await
        .unwrap();
        super::execute(
            project(
                "TWO".parse().unwrap(),
                "two",
                "/work/shared",
                "/tasks/two",
                home,
            ),
            &pool,
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
        let home = PathBuf::from("/home/tester");
        super::execute(
            project(
                "PWF".parse().unwrap(),
                "pwf",
                "/work/pwf",
                "/tasks/pwf",
                home.clone(),
            ),
            &pool,
        )
        .await
        .unwrap();

        let error = super::execute(
            project(
                "pwf".parse().unwrap(),
                "other",
                "/work/other",
                "/tasks/other",
                home,
            ),
            &pool,
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
        let home = PathBuf::from("/home/tester");
        super::execute(
            project(
                "PWF".parse().unwrap(),
                "pwf",
                "/work/pwf",
                "/tasks/pwf",
                home.clone(),
            ),
            &pool,
        )
        .await
        .unwrap();

        let error = super::execute(
            project(
                "ALT".parse().unwrap(),
                "pwf",
                "/work/other",
                "/tasks/other",
                home,
            ),
            &pool,
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
        let home = PathBuf::from("/home/tester");
        super::execute(
            project(
                "PWF".parse().unwrap(),
                "pwf",
                "/work/pwf",
                "/tasks/pwf",
                home.clone(),
            ),
            &pool,
        )
        .await
        .unwrap();

        let error = super::execute(
            project(
                "ALT".parse().unwrap(),
                "pwf",
                "/work/rolled-back",
                "/tasks/other",
                home,
            ),
            &pool,
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
        let home = PathBuf::from("/home/tester");
        let resolved_path = home.join("tasks/shared");
        insert_project(
            &pool,
            "PWF".parse().unwrap(),
            "pwf",
            "/work/PWF",
            "~/tasks/shared",
            true,
        )
        .await;

        let error = super::execute(
            project(
                "ALT".parse().unwrap(),
                "other",
                "/work/ALT",
                &resolved_path.to_string_lossy(),
                home,
            ),
            &pool,
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
