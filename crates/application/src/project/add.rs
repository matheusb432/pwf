use std::error::Error;

use pwf_domain::project::{ProjectName, ProjectPrefix, ProjectSource, ProjectTasks};
use sqlx::error::ErrorKind;

use super::{
    Project,
    dto::{ProjectRow, ProjectRowError},
};
use crate::AppDbStore;

/// Requests creation of one managed project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddProject {
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
    /// The pending-work task location already belongs to a project.
    #[error("project task location already exists")]
    DuplicateTaskLocation {
        /// Conflicting task location.
        tasks: ProjectTasks,
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
/// Returns a conflict variant when the ID, title, or task location exists. Returns
/// [`AddProjectError::Unexpected`] for database and persisted-data failures.
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
    let source_kind = command.source.kind().to_string();
    let source_value = command.source.value().as_ref();
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

    let id = command.id.as_ref();
    let title = command.title.as_ref();
    let tasks_kind = command.tasks.kind().to_string();
    let tasks_path = command.tasks.path().as_ref();
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

    let id = command.id.as_ref();
    let title = command.title.as_ref();
    let tasks_kind = command.tasks.kind().to_string();
    let tasks_path = command.tasks.path().as_ref();
    let conflicts = sqlx::query!(
        r#"
        SELECT
            EXISTS(SELECT 1 FROM projects WHERE id = ?) AS "id_exists!: bool",
            EXISTS(SELECT 1 FROM projects WHERE title = ?) AS "title_exists!: bool",
            EXISTS(
                SELECT 1
                FROM projects
                WHERE tasks_kind = ? AND tasks_path = ?
            ) AS "tasks_exist!: bool"
        "#,
        id,
        title,
        tasks_kind,
        tasks_path,
    )
    .fetch_one(&mut **transaction)
    .await;
    let conflicts = match conflicts {
        Ok(conflicts) => conflicts,
        Err(query_error) => return unexpected("classifying project conflict", query_error),
    };
    if conflicts.id_exists {
        return AddProjectError::DuplicateProjectId {
            id: command.id.clone(),
        };
    }
    if conflicts.title_exists {
        return AddProjectError::DuplicateProjectTitle {
            title: command.title.clone(),
        };
    }
    if conflicts.tasks_exist {
        return AddProjectError::DuplicateTaskLocation {
            tasks: command.tasks.clone(),
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
