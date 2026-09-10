use std::error::Error;

use pwf_models::project::ProjectId;
use pwf_wire::{patch_field::PatchField, project::UpdateProject, set_field::SetField};

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

/// Updates a managed project's configuration, preserving omitted fields.
#[cqrsy::command]
pub async fn execute(
    command: UpdateProject,
    pool: &sqlx::SqlitePool,
) -> Result<(), UpdateProjectError> {
    let mut transaction = pool
        .begin_with("BEGIN IMMEDIATE")
        .await
        .map_err(|error| unexpected("starting project update transaction", error))?;
    let (update_source, source_id) = match &command.source {
        PatchField::Set(source) => (
            true,
            Some(
                source_record::get_or_insert(&mut transaction, source)
                    .await
                    .map_err(|error| unexpected("resolving project source", error))?,
            ),
        ),
        PatchField::Clear => (true, None),
        PatchField::NoAction => (false, None),
    };
    let project_id = command.id.as_ref();
    let (update_vault, obsidian_vault) = match &command.obsidian_vault {
        pwf_wire::patch_field::PatchField::NoAction => (false, None),
        pwf_wire::patch_field::PatchField::Clear => (true, None),
        pwf_wire::patch_field::PatchField::Set(value) => (true, Some(value.as_ref())),
    };
    let snapshot_enabled = match command.snapshot_enabled {
        SetField::NoAction => None,
        SetField::Set(value) => Some(value),
    };
    let update = sqlx::query!(
        "UPDATE projects SET project_source_id = CASE WHEN ? THEN ? ELSE project_source_id END, obsidian_vault = CASE WHEN ? THEN ? ELSE obsidian_vault END, snapshot_enabled = COALESCE(?, snapshot_enabled) WHERE id = ?",
        update_source,
        source_id,
        update_vault,
        obsidian_vault,
        snapshot_enabled,
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
