use std::{error::Error, path::PathBuf};

use pwf_domain::project::{ProjectName, ProjectPrefix};

use super::{
    Project,
    add_project::AddProjectFields,
    dto::{ProjectRow, ProjectRowError},
    task_location::{self, TaskLocationError},
};
use crate::AppDbStore;

/// Requests replacement of one managed project's identity and locations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameProject {
    /// Existing project prefix.
    pub current_id: ProjectPrefix,
    /// Replacement project fields.
    pub fields: AddProjectFields,
    /// Home directory used to expand home-relative task paths.
    pub home: PathBuf,
}

/// Reports an expected conflict or unexpected project rename failure.
#[derive(Debug, thiserror::Error)]
pub enum RenameProjectError {
    /// The source project does not exist.
    #[error("project not found: {id}")]
    SourceProjectNotFound {
        /// Missing source project prefix.
        id: ProjectPrefix,
    },
    /// The destination project prefix belongs to another project.
    #[error("project id already exists: {id}")]
    DestinationProjectIdExists {
        /// Conflicting project prefix.
        id: ProjectPrefix,
    },
    /// The destination project title belongs to another project.
    #[error("project title already exists: {title}")]
    DestinationProjectTitleExists {
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
    /// Project rename failed outside an expected conflict.
    #[error("{context}: {source}")]
    Unexpected {
        /// Failed operation boundary.
        context: &'static str,
        /// Concrete database or persisted-data failure.
        #[source]
        source: Box<dyn Error + Send + Sync>,
    },
}

/// Replaces one managed project's identity and locations in one immediate transaction.
#[cqrsy::command]
pub async fn execute(
    command: RenameProject,
    database: &impl AppDbStore,
) -> Result<Project, RenameProjectError> {
    let mut transaction = database
        .pool()
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|error| unexpected("starting project rename transaction", error))?;
    validate_identity(&mut transaction, &command).await?;
    let current_id = command.current_id.as_ref();
    let destination_id = command.fields.id.as_ref();
    let destination_title = command.fields.title.as_ref();
    let existing = other_task_locations(&mut transaction, &command.current_id).await?;
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
    .map_err(|error| unexpected("reading destination project source", error))?;
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
        .map_err(|error| unexpected("inserting destination project source", error))?
        .last_insert_rowid(),
    };

    let tasks_kind = command.fields.tasks.kind().to_string();
    let tasks_path = command.fields.tasks.path().as_ref();
    sqlx::query!(
        r#"
        UPDATE projects
        SET
            id = ?,
            project_source_id = ?,
            title = ?,
            tasks_kind = ?,
            tasks_path = ?
        WHERE id = ?
        "#,
        destination_id,
        source_id,
        destination_title,
        tasks_kind,
        tasks_path,
        current_id,
    )
    .execute(&mut *transaction)
    .await
    .map_err(|error| unexpected("updating project", error))?;

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
        destination_id,
    )
    .fetch_one(&mut *transaction)
    .await
    .map_err(|error| unexpected("reading renamed project", error))?;
    let project = Project::try_from(row)
        .map_err(|error| unexpected_row("converting renamed project", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| unexpected("committing project rename", error))?;
    Ok(project)
}

async fn validate_identity(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    command: &RenameProject,
) -> Result<(), RenameProjectError> {
    let current_id = command.current_id.as_ref();
    let source_exists = sqlx::query_scalar!(
        r#"
        SELECT EXISTS(
            SELECT 1
            FROM projects
            WHERE id = ?
        ) AS "exists!: bool"
        "#,
        current_id,
    )
    .fetch_one(&mut **transaction)
    .await
    .map_err(|error| unexpected("reading source project", error))?;
    if !source_exists {
        return Err(RenameProjectError::SourceProjectNotFound {
            id: command.current_id.clone(),
        });
    }

    let destination_id = command.fields.id.as_ref();
    let destination_title = command.fields.title.as_ref();
    let conflicts = sqlx::query!(
        r#"
        SELECT
            EXISTS(
                SELECT 1
                FROM projects
                WHERE id = ? AND id != ?
            ) AS "id_exists!: bool",
            EXISTS(
                SELECT 1
                FROM projects
                WHERE title = ? AND id != ?
            ) AS "title_exists!: bool"
        "#,
        destination_id,
        current_id,
        destination_title,
        current_id,
    )
    .fetch_one(&mut **transaction)
    .await
    .map_err(|error| unexpected("classifying project rename conflicts", error))?;
    if conflicts.id_exists {
        return Err(RenameProjectError::DestinationProjectIdExists {
            id: command.fields.id.clone(),
        });
    }
    if conflicts.title_exists {
        return Err(RenameProjectError::DestinationProjectTitleExists {
            title: command.fields.title.clone(),
        });
    }
    Ok(())
}

async fn other_task_locations(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    current_id: &ProjectPrefix,
) -> Result<Vec<(ProjectPrefix, String)>, RenameProjectError> {
    sqlx::query_as::<_, (String, String)>(
        "SELECT id, tasks_path FROM projects WHERE id != ? ORDER BY id ASC",
    )
    .bind(current_id.as_ref())
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

fn task_location_error(error: TaskLocationError) -> RenameProjectError {
    match error {
        TaskLocationError::InvalidPath {
            project_id,
            path,
            source,
        } => RenameProjectError::InvalidTaskPath {
            project_id,
            path,
            source,
        },
        TaskLocationError::Collision {
            first_id,
            second_id,
            path,
        } => RenameProjectError::DuplicateRuntimeTaskLocation {
            first_id,
            second_id,
            path,
        },
    }
}

fn unexpected(
    context: &'static str,
    source: impl Error + Send + Sync + 'static,
) -> RenameProjectError {
    RenameProjectError::Unexpected {
        context,
        source: Box::new(source),
    }
}

fn unexpected_row(context: &'static str, source: ProjectRowError) -> RenameProjectError {
    unexpected(context, source)
}

#[cfg(test)]
mod tests {
    use std::{assert_matches, path::PathBuf};

    use pwf_domain::project::{
        ProjectName, ProjectPrefix, ProjectSource, ProjectSourceKind, ProjectSourceValue,
        ProjectTasks, ProjectTasksKind, ProjectTasksPath,
    };

    use crate::{
        AppDbStore,
        ports::TestDatabase,
        project::{
            add_project::AddProjectFields,
            rename_project::{self, RenameProject, RenameProjectError},
        },
    };

    fn fields(id: &str, title: &str, source: &str, tasks: &str) -> AddProjectFields {
        AddProjectFields {
            id: ProjectPrefix::try_new(id).unwrap(),
            title: ProjectName::try_new(title).unwrap(),
            source: ProjectSource::new(
                ProjectSourceKind::Directory,
                ProjectSourceValue::try_new(source).unwrap(),
            ),
            tasks: ProjectTasks::new(
                ProjectTasksKind::Directory,
                ProjectTasksPath::try_new(tasks).unwrap(),
            ),
        }
    }

    #[tokio::test]
    async fn rename_replaces_identity_and_preserves_project_state() {
        let database = TestDatabase::new().await;
        database
            .insert_project(
                "SSH",
                "ssh-agent-phone-app",
                "/self/ssh-agent-phone-app",
                "/pwf-db/self/ssh-agent-phone-app",
                true,
            )
            .await;

        let renamed = rename_project::execute(
            RenameProject {
                current_id: ProjectPrefix::try_new("SSH").unwrap(),
                fields: fields("MUX", "mimux", "/self/mimux", "/pwf-db/self/mimux"),
                home: PathBuf::from("/home/tester"),
            },
            &database,
        )
        .await
        .unwrap();

        assert_eq!(renamed.id.as_ref(), "MUX");
        assert_eq!(renamed.title.as_ref(), "mimux");
        assert_eq!(renamed.source.value().as_ref(), "/self/mimux");
        assert_eq!(renamed.tasks.path().as_ref(), "/pwf-db/self/mimux");
        assert_eq!(renamed.created_at, "2026-07-26T00:00:00.000Z");
        assert!(renamed.is_paused);
    }

    #[tokio::test]
    async fn missing_source_project_is_classified() {
        let database = TestDatabase::new().await;

        let error = rename_project::execute(
            RenameProject {
                current_id: ProjectPrefix::try_new("SSH").unwrap(),
                fields: fields("MUX", "mimux", "/self/mimux", "/pwf-db/self/mimux"),
                home: PathBuf::from("/home/tester"),
            },
            &database,
        )
        .await
        .unwrap_err();

        assert_matches!(
            error,
            RenameProjectError::SourceProjectNotFound { id }
                if id == ProjectPrefix::try_new("SSH").unwrap()
        );
    }

    #[tokio::test]
    async fn destination_id_conflict_leaves_source_unchanged() {
        let database = TestDatabase::new().await;
        database
            .insert_project(
                "SSH",
                "ssh-agent-phone-app",
                "/self/ssh-agent-phone-app",
                "/pwf-db/self/ssh-agent-phone-app",
                false,
            )
            .await;
        database
            .insert_project("MUX", "other", "/self/other", "/pwf-db/self/other", false)
            .await;

        let error = rename_project::execute(
            RenameProject {
                current_id: ProjectPrefix::try_new("SSH").unwrap(),
                fields: fields("MUX", "mimux", "/self/mimux", "/pwf-db/self/mimux"),
                home: PathBuf::from("/home/tester"),
            },
            &database,
        )
        .await
        .unwrap_err();

        assert_matches!(
            error,
            RenameProjectError::DestinationProjectIdExists { id }
                if id == ProjectPrefix::try_new("MUX").unwrap()
        );
        let source: (String, String, String) =
            sqlx::query_as("SELECT id, title, tasks_path FROM projects WHERE id = 'SSH'")
                .fetch_one(database.pool())
                .await
                .unwrap();
        assert_eq!(
            source,
            (
                "SSH".to_string(),
                "ssh-agent-phone-app".to_string(),
                "/pwf-db/self/ssh-agent-phone-app".to_string(),
            )
        );
    }

    #[tokio::test]
    async fn destination_title_conflict_is_classified() {
        let database = TestDatabase::new().await;
        database
            .insert_project(
                "SSH",
                "ssh-agent-phone-app",
                "/self/ssh-agent-phone-app",
                "/pwf-db/self/ssh-agent-phone-app",
                false,
            )
            .await;
        database
            .insert_project("ALT", "mimux", "/self/other", "/pwf-db/self/other", false)
            .await;

        let error = rename_project::execute(
            RenameProject {
                current_id: ProjectPrefix::try_new("SSH").unwrap(),
                fields: fields("MUX", "mimux", "/self/mimux", "/pwf-db/self/mimux"),
                home: PathBuf::from("/home/tester"),
            },
            &database,
        )
        .await
        .unwrap_err();

        assert_matches!(
            error,
            RenameProjectError::DestinationProjectTitleExists { title }
                if title == ProjectName::try_new("mimux").unwrap()
        );
    }

    #[tokio::test]
    async fn runtime_task_collision_leaves_source_unchanged() {
        let database = TestDatabase::new().await;
        database
            .insert_project(
                "SSH",
                "ssh-agent-phone-app",
                "/self/ssh-agent-phone-app",
                "/pwf-db/self/ssh-agent-phone-app",
                false,
            )
            .await;
        database
            .insert_project("ALT", "other", "/self/other", "~/tasks/shared", false)
            .await;

        let error = rename_project::execute(
            RenameProject {
                current_id: ProjectPrefix::try_new("SSH").unwrap(),
                fields: fields("MUX", "mimux", "/self/mimux", "/home/tester/tasks/shared"),
                home: PathBuf::from("/home/tester"),
            },
            &database,
        )
        .await
        .unwrap_err();

        assert_matches!(
            error,
            RenameProjectError::DuplicateRuntimeTaskLocation {
                first_id,
                second_id,
                ..
            } if first_id == ProjectPrefix::try_new("ALT").unwrap()
                && second_id == ProjectPrefix::try_new("MUX").unwrap()
        );
        let title: String = sqlx::query_scalar("SELECT title FROM projects WHERE id = 'SSH'")
            .fetch_one(database.pool())
            .await
            .unwrap();
        assert_eq!(title, "ssh-agent-phone-app");
    }

    #[tokio::test]
    async fn invalid_task_path_is_classified() {
        let database = TestDatabase::new().await;
        database
            .insert_project(
                "SSH",
                "ssh-agent-phone-app",
                "/self/ssh-agent-phone-app",
                "/pwf-db/self/ssh-agent-phone-app",
                false,
            )
            .await;

        let error = rename_project::execute(
            RenameProject {
                current_id: ProjectPrefix::try_new("SSH").unwrap(),
                fields: fields("MUX", "mimux", "/self/mimux", "~/tasks/../mimux"),
                home: PathBuf::from("/home/tester"),
            },
            &database,
        )
        .await
        .unwrap_err();

        assert_matches!(error, RenameProjectError::InvalidTaskPath { .. });
    }
}
