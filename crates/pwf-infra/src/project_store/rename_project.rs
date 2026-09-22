use std::error::Error;

use pwf_application::project::rename_project::RenameProjectError;
use pwf_models::project::{HomeDirectory, Project, ProjectId, ProjectTasksPath};
use pwf_wire::project::RenameProject;
use sqlx::Connection as _;

use super::{source_record, task_location};
pub(super) async fn execute(
    command: RenameProject,
    expected_current: &Project,
    connection: &mut sqlx::SqliteConnection,
    home: &HomeDirectory,
) -> Result<Project, RenameProjectError> {
    let mut transaction = connection
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|error| unexpected("starting project rename transaction", error))?;
    validate_identity(&mut transaction, &command, expected_current).await?;
    let existing = other_task_locations(&mut transaction, &command.current_id).await?;
    task_location::reject_collision(
        &command.fields.id,
        command.fields.tasks.path(),
        existing,
        home,
    )?;
    let source_id = match &command.fields.source {
        Some(source) => Some(
            source_record::get_or_insert(&mut transaction, source)
                .await
                .map_err(|error| unexpected("resolving destination project source", error))?,
        ),
        None => None,
    };
    let row = replace_project_row(&mut transaction, &command, source_id).await?;
    let project = super::project_from_row(row)
        .map_err(|error| unexpected("converting renamed project", error))?;
    transaction
        .commit()
        .await
        .map_err(|error| unexpected("committing project rename", error))?;
    Ok(project)
}

async fn replace_project_row(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    command: &RenameProject,
    source_id: Option<i64>,
) -> Result<super::ProjectRow, RenameProjectError> {
    let current_id = command.current_id.as_ref();
    let destination_id = command.fields.id.as_ref();
    let destination_title = command.fields.title.as_ref();
    let tasks_kind = command.fields.tasks.kind().to_string();
    let tasks_path = command.fields.tasks.path().as_ref();
    let obsidian_vault = command.fields.obsidian_vault.as_ref().map(AsRef::as_ref);
    let rows = project_returning!(
        r#"
        UPDATE projects
        SET
            id = ?,
            project_source_id = ?,
            title = ?,
            tasks_kind = ?,
            tasks_path = ?,
            obsidian_vault = ?
        WHERE id = ?
        "#,
        destination_id,
        source_id,
        destination_title,
        tasks_kind,
        tasks_path,
        obsidian_vault,
        current_id,
    )
    .fetch_all(&mut **transaction)
    .await
    .map_err(|error| unexpected("updating project", error))?;
    match rows.len() {
        1 => rows.into_iter().next().ok_or_else(|| {
            unexpected(
                "updating project",
                std::io::Error::other("project rename returned no row"),
            )
        }),
        0 => Err(RenameProjectError::SourceProjectNotFound {
            id: command.current_id.clone(),
        }),
        count => Err(unexpected(
            "updating project",
            std::io::Error::other(format!(
                "project rename updated {count} rows; expected exactly one"
            )),
        )),
    }
}

async fn validate_identity(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    command: &RenameProject,
    expected_current: &Project,
) -> Result<(), RenameProjectError> {
    let current_id = command.current_id.as_ref();
    let current_row = project_query!("WHERE projects.id = ?", current_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(|error| unexpected("reading source project", error))?;
    let Some(current_row) = current_row else {
        return Err(RenameProjectError::SourceProjectNotFound {
            id: command.current_id.clone(),
        });
    };
    let current = super::project_from_row(current_row)
        .map_err(|error| unexpected("converting source project", error))?;
    if current != *expected_current {
        return Err(RenameProjectError::SourceProjectChanged {
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
    current_id: &ProjectId,
) -> Result<Vec<(ProjectId, ProjectTasksPath)>, RenameProjectError> {
    sqlx::query_as::<_, (String, String)>(
        "SELECT id, tasks_path FROM projects WHERE id != ? ORDER BY id ASC",
    )
    .bind(current_id.as_ref())
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

fn unexpected(
    context: &'static str,
    source: impl Error + Send + Sync + 'static,
) -> RenameProjectError {
    RenameProjectError::Unexpected {
        context,
        source: anyhow::Error::new(source),
    }
}
