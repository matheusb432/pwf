use std::{error::Error, path::PathBuf};

use pwf_domain::project::{ProjectName, ProjectPrefix, ProjectSource, ProjectTasks};
use sqlx::error::ErrorKind;

use super::{
    Project,
    dto::{ProjectRow, ProjectRowError},
    task_location::{self, TaskLocationError},
};
use crate::AppDbStore;

/// Requests creation of one managed project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddProject {
    /// Project fields parsed from user input.
    pub fields: AddProjectFields,
    /// Home directory used to expand home-relative task paths.
    pub home: PathBuf,
}

/// Defines the persisted fields for one managed project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddProjectFields {
    /// Canonical project prefix.
    pub id: ProjectPrefix,
    /// Unique project title.
    pub title: ProjectName,
    /// Source location reused across matching projects.
    pub source: ProjectSource,
    /// Unique pending-work task location.
    pub tasks: ProjectTasks,
}

/// Reports an expected conflict or unexpected project creation failure.
#[derive(Debug, thiserror::Error)]
pub enum AddProjectError {
    /// The project prefix already exists.
    #[error("project id already exists: {id}")]
    DuplicateProjectId {
        /// Conflicting project prefix.
        id: ProjectPrefix,
    },
    /// The project title already exists.
    #[error("project title already exists: {title}")]
    DuplicateProjectTitle {
        /// Conflicting project title.
        title: ProjectName,
    },
    /// A task path cannot be resolved for the current runtime.
    #[error("managed project {project_id} task path '{path}' is invalid: {source}")]
    InvalidTaskPath {
        /// Project containing the invalid task path.
        project_id: ProjectPrefix,
        /// Persisted task path value.
        path: String,
        /// Path validation failure.
        #[source]
        source: super::resolve_runtime_path::RuntimePathError,
    },
    /// Two projects resolve to the same runtime task location.
    #[error(
        "managed projects {first_id} and {second_id} resolve to the same task location: {}",
        path.display()
    )]
    DuplicateRuntimeTaskLocation {
        /// First project prefix in lexical order.
        first_id: ProjectPrefix,
        /// Second project prefix in lexical order.
        second_id: ProjectPrefix,
        /// Conflicting resolved task location.
        path: PathBuf,
    },
    /// Project creation failed outside an expected conflict.
    #[error("{context}: {source}")]
    Unexpected {
        /// Failed operation boundary.
        context: &'static str,
        /// Concrete database or persisted-data failure.
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
    database: &impl AppDbStore,
) -> Result<Project, AddProjectError> {
    let mut transaction = database
        .pool()
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
    let project = Project::try_from(row)
        .map_err(|error| unexpected_row("converting created project", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| unexpected("committing project creation", error))?;
    Ok(project)
}

async fn other_task_locations(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    candidate_id: &ProjectPrefix,
) -> Result<Vec<(ProjectPrefix, String)>, AddProjectError> {
    sqlx::query_as::<_, (String, String)>(
        "SELECT id, tasks_path FROM projects WHERE id != ? ORDER BY id ASC",
    )
    .bind(candidate_id.as_ref())
    .fetch_all(&mut **transaction)
    .await
    .map_err(|error| unexpected("reading project task locations", error))?
    .into_iter()
    .map(|(id, tasks_path)| {
        ProjectPrefix::try_new(id)
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

    use pwf_domain::project::{
        ProjectName, ProjectPrefix, ProjectSource, ProjectSourceKind, ProjectSourceValue,
        ProjectTasks, ProjectTasksKind, ProjectTasksPath,
    };
    use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};

    use super::*;

    #[derive(Clone)]
    struct TestDatabase(SqlitePool);

    impl AppDbStore for TestDatabase {
        fn pool(&self) -> &SqlitePool {
            &self.0
        }
    }

    async fn database() -> TestDatabase {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE project_sources (id INTEGER PRIMARY KEY, kind TEXT NOT NULL, value TEXT NOT NULL, created_at TEXT NOT NULL DEFAULT '2026-07-26T00:00:00.000Z', UNIQUE (kind, value))",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE projects (id TEXT PRIMARY KEY, project_source_id INTEGER NOT NULL, title TEXT NOT NULL UNIQUE, tasks_kind TEXT NOT NULL, tasks_path TEXT NOT NULL, created_at TEXT NOT NULL DEFAULT '2026-07-26T00:00:00.000Z', paused_at TEXT, UNIQUE (tasks_kind, tasks_path))",
        )
        .execute(&pool)
        .await
        .unwrap();
        TestDatabase(pool)
    }

    async fn insert_paused_project(database: &TestDatabase, id: &str, tasks_path: &str) {
        let source_id =
            sqlx::query("INSERT INTO project_sources (kind, value) VALUES ('directory', ?)")
                .bind(format!("/work/{id}"))
                .execute(database.pool())
                .await
                .unwrap()
                .last_insert_rowid();
        sqlx::query(
            "INSERT INTO projects (id, project_source_id, title, tasks_kind, tasks_path, paused_at) VALUES (?, ?, ?, 'directory', ?, '2026-07-26T00:00:00.000Z')",
        )
        .bind(id)
        .bind(source_id)
        .bind(id.to_ascii_lowercase())
        .bind(tasks_path)
        .execute(database.pool())
        .await
        .unwrap();
    }

    fn project(id: &str, title: &str, tasks_path: &str, home: PathBuf) -> AddProject {
        AddProject {
            fields: AddProjectFields {
                id: ProjectPrefix::try_new(id).unwrap(),
                title: ProjectName::try_new(title).unwrap(),
                source: ProjectSource::new(
                    ProjectSourceKind::Directory,
                    ProjectSourceValue::try_new(format!("/work/{id}")).unwrap(),
                ),
                tasks: ProjectTasks::new(
                    ProjectTasksKind::Directory,
                    ProjectTasksPath::try_new(tasks_path).unwrap(),
                ),
            },
            home,
        }
    }

    #[tokio::test]
    async fn runtime_alias_of_paused_project_is_rejected() {
        let database = database().await;
        let home = PathBuf::from("/home/developer");
        let resolved_path = home.join("tasks/shared");
        insert_paused_project(&database, "PWF", "~/tasks/shared").await;

        let error = super::execute(
            project("ALT", "other", &resolved_path.to_string_lossy(), home),
            &database,
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
    }
}
