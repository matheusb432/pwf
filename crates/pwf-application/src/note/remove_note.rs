//! Removes one note from a managed project.

use pwf_models::{
    note::NoteSelector,
    project::{ProjectId, ProjectName, ProjectSelector},
};
use pwf_wire::{note::RemovedNote, project::ProjectStatusFilter};

use crate::{
    ports::project_note::ProjectNoteStore,
    project::resolve_project::{self, ResolveProject, ResolveProjectError},
};

/// Requests deletion of one project note.
#[derive(Debug, Clone)]
pub struct RemoveNote {
    /// Selects the managed project by name or id code.
    pub project_selector: ProjectSelector,
    /// Selects the note by full id, `NOTE-NNNN`, or bare numeric suffix.
    pub selector: NoteSelector,
}

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
    #[error("{0}")]
    Store(#[source] Box<dyn std::error::Error + Send + Sync>),
}

/// Resolves and deletes one note after confirming its representation exists.
///
/// # Errors
///
/// Returns [`RemoveNoteError::ResolveProject`] when the project does not resolve,
/// [`RemoveNoteError::ProjectMismatch`] when the note id names another project,
/// [`RemoveNoteError::NoSuchNote`] when the note does not exist, or
/// [`RemoveNoteError::Store`] when existence inspection or deletion fails.
#[cqrsy::command]
pub async fn execute(
    command: RemoveNote,
    store: &impl ProjectNoteStore,
    pool: &sqlx::SqlitePool,
) -> Result<RemovedNote, RemoveNoteError> {
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
        .map_err(|error| RemoveNoteError::Store(Box::new(error)))?;
    if !exists {
        return Err(RemoveNoteError::NoSuchNote {
            id,
            project: project.title,
        });
    }
    store
        .delete_note(&project, &id)
        .map_err(|error| RemoveNoteError::Store(Box::new(error)))?;
    Ok(RemovedNote { id })
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use pwf_models::{
        note::{NoteId, NoteTitle, ProjectNote},
        project::ProjectId,
    };

    use super::{RemoveNote, RemoveNoteError};
    use crate::testing::{InMemoryStore, ProjectNoteFailure, insert_project};

    #[derive(Debug, thiserror::Error)]
    #[error("sentinel store failure")]
    struct SentinelStoreError;

    fn note() -> ProjectNote {
        ProjectNote {
            id: NoteId::try_new("PWF-NOTE-0007").unwrap(),
            title: NoteTitle::try_new("remember milk").unwrap(),
        }
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn full_prefixless_and_bare_identifiers_resolve(pool: sqlx::SqlitePool) {
        insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
        for identifier in [
            "PWF-NOTE-0007",
            concat!("pwf", "-note-0007"),
            "note-0007",
            "7",
        ] {
            let store = InMemoryStore::default().with_project_notes("pwf", vec![note()]);

            let removed = super::execute(
                RemoveNote {
                    project_selector: "PWF".parse().unwrap(),
                    selector: identifier.parse().unwrap(),
                },
                &store,
                &pool,
            )
            .await
            .unwrap();

            assert_eq!(removed.id.as_ref(), "PWF-NOTE-0007");
            assert!(store.project_notes("pwf").is_empty());
        }
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn missing_note_wins_over_adapter_delete_failure(pool: sqlx::SqlitePool) {
        insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
        let store = InMemoryStore::default().with_failure(ProjectNoteFailure::Delete);

        let error = super::execute(
            RemoveNote {
                project_selector: "pwf".parse().unwrap(),
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
            } if id.as_ref() == "PWF-NOTE-0001" && project.as_ref() == "pwf"
        ));
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn identifier_from_another_project_is_rejected(pool: sqlx::SqlitePool) {
        insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
        let store = InMemoryStore::default();

        let error = super::execute(
            RemoveNote {
                project_selector: "pwf".parse().unwrap(),
                selector: "FOO-NOTE-0001".parse().unwrap(),
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
            } if selector.to_string() == "FOO-NOTE-0001"
                && project_id == &ProjectId::try_new("PWF").unwrap()
        ));
    }

    #[test]
    fn store_error_preserves_display_and_source() {
        let error = RemoveNoteError::Store(Box::new(SentinelStoreError));

        assert_eq!(error.to_string(), "sentinel store failure");
        let source = error.source().expect("store error retains its source");
        assert!(source.downcast_ref::<SentinelStoreError>().is_some());
        assert_eq!(source.to_string(), "sentinel store failure");
    }
}
