use std::error::Error;

use pwf_models::project::{HomeDirectory, ProjectId, ProjectName, ProjectTasksPath};
use pwf_wire::project::ProjectFields;
use sqlx::error::ErrorKind;

use super::{Project, TaskLocationError, source_record, task_location};

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
    let source_id = match &fields.source {
        Some(source) => Some(
            source_record::get_or_insert(&mut transaction, source)
                .await
                .map_err(|error| unexpected("resolving project source", error))?,
        ),
        None => None,
    };

    let id = fields.id.as_ref();
    let title = fields.title.as_ref();
    let tasks_kind = fields.tasks.kind().to_string();
    let tasks_path = fields.tasks.path().as_ref();
    let obsidian_vault = fields.obsidian_vault.as_ref().map(AsRef::as_ref);
    let snapshot_enabled = fields.snapshot_enabled;
    let insert_result = sqlx::query!(
        r#"
        INSERT INTO projects (id, project_source_id, title, tasks_kind, tasks_path, obsidian_vault, snapshot_enabled)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
        id,
        source_id,
        title,
        tasks_kind,
        tasks_path,
        obsidian_vault,
        snapshot_enabled,
    )
    .execute(&mut *transaction)
    .await;
    if let Err(error) = insert_result {
        return Err(project_insertion_error(&mut transaction, &fields, error).await);
    }

    let row = project_query!("WHERE projects.id = ?", id)
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
