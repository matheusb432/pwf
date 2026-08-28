//! Removes one note from a managed project.

use pwf_models::{
    note::{NoteId, NoteSelector},
    project::{ProjectId, ProjectName},
};
use pwf_wire::{
    note::RemoveNote,
    project::{ProjectStatusFilter, ResolveProject},
};

use crate::{
    ports::project_note::ProjectNoteStore,
    project::resolve_project::{self, ResolveProjectError},
};

#[derive(Debug, thiserror::Error)]
pub enum RemoveNoteError {
    #[error(transparent)]
    ResolveProject(#[from] ResolveProjectError),
    #[error("Note id '{selector}' does not belong to project {project_id}.")]
    ProjectMismatch {
        selector: NoteSelector,
        project_id: ProjectId,
    },
    #[error("No such note {id} in {project}.")]
    NoSuchNote {
        id: pwf_models::note::NoteId,
        project: ProjectName,
    },
    #[error(transparent)]
    Store(anyhow::Error),
}

/// Resolves and deletes one note after confirming its representation exists.
#[cqrsy::command]
pub async fn execute(
    command: RemoveNote,
    store: &impl ProjectNoteStore,
    pool: &sqlx::SqlitePool,
) -> Result<NoteId, RemoveNoteError> {
    let project = resolve_project::execute(
        ResolveProject {
            selector: command.project_selector,
            status: ProjectStatusFilter::ActiveOnly,
        },
        pool,
    )
    .await?;
    let id =
        command
            .selector
            .resolve(&project.id)
            .ok_or_else(|| RemoveNoteError::ProjectMismatch {
                selector: command.selector,
                project_id: project.id.clone(),
            })?;
    let exists = store
        .note_exists(&project, &id)
        .map_err(|error| RemoveNoteError::Store(anyhow::Error::new(error)))?;
    if !exists {
        return Err(RemoveNoteError::NoSuchNote {
            id,
            project: project.title,
        });
    }
    store
        .delete_note(&project, &id)
        .map_err(|error| RemoveNoteError::Store(anyhow::Error::new(error)))?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use pwf_models::{
        note::{NoteId, NoteTitle, ProjectNote},
        project::ProjectId,
    };

    use super::{RemoveNote, RemoveNoteError};
    use crate::{
        note::remove_note,
        testing::{InMemoryStore, ProjectNoteFailure, insert_project},
    };

    #[derive(Debug, thiserror::Error)]
    #[error("sentinel store failure")]
    struct SentinelStoreError;

    fn note() -> ProjectNote {
        ProjectNote {
            id: NoteId::try_new("FOO-NOTE-0007").unwrap(),
            title: NoteTitle::try_new("remember milk").unwrap(),
        }
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn full_prefixless_and_bare_identifiers_resolve(pool: sqlx::SqlitePool) {
        insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
        for identifier in [
            "FOO-NOTE-0007",
            concat!("foo", "-note-0007"),
            "note-0007",
            "7",
        ] {
            let store = InMemoryStore::default().with_project_notes("foo", vec![note()]);

            let removed = remove_note::execute(
                RemoveNote {
                    project_selector: "FOO".parse().unwrap(),
                    selector: identifier.parse().unwrap(),
                },
                &store,
                &pool,
            )
            .await
            .unwrap();

            assert_eq!(removed.as_ref(), "FOO-NOTE-0007");
            assert!(store.project_notes("foo").is_empty());
        }
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn missing_note_wins_over_adapter_delete_failure(pool: sqlx::SqlitePool) {
        insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
        let store = InMemoryStore::default().with_failure(ProjectNoteFailure::Delete);

        let error = remove_note::execute(
            RemoveNote {
                project_selector: "foo".parse().unwrap(),
                selector: "1".parse().unwrap(),
            },
            &store,
            &pool,
        )
        .await
        .unwrap_err();

        assert!(matches!(
            error,
            RemoveNoteError::NoSuchNote {
                ref id,
                ref project,
            } if id.as_ref() == "FOO-NOTE-0001" && project.as_ref() == "foo"
        ));
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn identifier_from_another_project_is_rejected(pool: sqlx::SqlitePool) {
        insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
        let store = InMemoryStore::default();

        let error = remove_note::execute(
            RemoveNote {
                project_selector: "foo".parse().unwrap(),
                selector: "BAR-NOTE-0001".parse().unwrap(),
            },
            &store,
            &pool,
        )
        .await
        .unwrap_err();

        assert!(matches!(
            error,
            RemoveNoteError::ProjectMismatch {
                ref selector,
                ref project_id,
            } if selector.to_string() == "BAR-NOTE-0001"
                && project_id == &ProjectId::try_new("FOO").unwrap()
        ));
    }

    #[test]
    fn store_error_preserves_display_and_root_cause() {
        let error = RemoveNoteError::Store(anyhow::Error::new(SentinelStoreError));

        assert_eq!(error.to_string(), "sentinel store failure");
        let source = match error {
            RemoveNoteError::Store(source) => Some(source),
            _ => None,
        };
        assert!(source.is_some());
        let source = source.unwrap();
        assert!(source.downcast_ref::<SentinelStoreError>().is_some());
        assert_eq!(source.root_cause().to_string(), "sentinel store failure");
    }
}
