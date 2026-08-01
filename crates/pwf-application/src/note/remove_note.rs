//! Removes one note from a managed project.

use pwf_models::{note::NoteId, project::Project};

use super::logic;
use crate::{
    ports::project_note::ProjectNoteStore,
    project::{
        ProjectStatusFilter,
        resolve_project::{self, ResolveProject, ResolveProjectError},
    },
};

/// Requests deletion of one project note.
#[derive(Debug, Clone)]
pub struct RemoveNote {
    /// Selects the managed project by name or id code.
    pub project_identifier: String,
    /// Selects the note by full id, `NOTE-NNNN`, or bare numeric suffix.
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoveNoteOk {
    pub id: NoteId,
}

#[derive(Debug, thiserror::Error)]
pub enum RemoveNoteError {
    #[error("Unknown project '{identifier}'; expected a managed project name or id code.")]
    UnknownProject { identifier: String },
    #[error("Invalid note id '{id}'; expected e.g. {project_id}-NOTE-0001, NOTE-0001, or 1.")]
    InvalidIdentifier { id: String, project_id: String },
    #[error("No such note {id} in {project}.")]
    NoSuchNote { id: String, project: String },
    #[error("{0}")]
    Project(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("{0}")]
    Store(#[source] Box<dyn std::error::Error + Send + Sync>),
}

/// Resolves and deletes one note after confirming its representation exists.
///
/// # Errors
///
/// Returns [`RemoveNoteError::UnknownProject`] when the project does not resolve,
/// [`RemoveNoteError::InvalidIdentifier`] when the note id is invalid for that project,
/// [`RemoveNoteError::NoSuchNote`] when the note does not exist, or
/// [`RemoveNoteError::Store`] when existence inspection or deletion fails.
#[cqrsy::command]
pub async fn execute(
    command: RemoveNote,
    store: &impl ProjectNoteStore,
    pool: &sqlx::SqlitePool,
) -> Result<RemoveNoteOk, RemoveNoteError> {
    let identifier = command.project_identifier.clone();
    let project = resolve_project::execute(
        ResolveProject {
            identifier: identifier.clone(),
            status: ProjectStatusFilter::ACTIVE,
        },
        pool,
    )
    .await
    .map_err(|error| project_error(identifier, error))?;
    execute_for_project(command, store, &project)
}

fn execute_for_project(
    command: RemoveNote,
    store: &impl ProjectNoteStore,
    project: &Project,
) -> Result<RemoveNoteOk, RemoveNoteError> {
    let raw_id = command.id;
    let id = logic::resolve_note(&raw_id, &project.id).ok_or_else(|| {
        RemoveNoteError::InvalidIdentifier {
            id: raw_id,
            project_id: project.id.to_string(),
        }
    })?;
    let exists = store
        .note_exists(project, &id)
        .map_err(|error| RemoveNoteError::Store(Box::new(error)))?;
    if !exists {
        return Err(RemoveNoteError::NoSuchNote {
            id: id.to_string(),
            project: project.title.to_string(),
        });
    }
    store
        .delete_note(project, &id)
        .map_err(|error| RemoveNoteError::Store(Box::new(error)))?;
    Ok(RemoveNoteOk { id })
}

fn project_error(identifier: String, error: ResolveProjectError) -> RemoveNoteError {
    match error {
        ResolveProjectError::Unknown { .. } => RemoveNoteError::UnknownProject { identifier },
        error @ ResolveProjectError::Unexpected { .. } => RemoveNoteError::Project(Box::new(error)),
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use pwf_models::note::{NoteId, ProjectNote};

    use super::{RemoveNote, RemoveNoteError};
    use crate::testing::{InMemoryStore, ProjectNoteFailure, project};

    #[derive(Debug, thiserror::Error)]
    #[error("sentinel store failure")]
    struct SentinelStoreError;

    fn note() -> ProjectNote {
        ProjectNote {
            id: NoteId::try_new("PWF-NOTE-0007").unwrap(),
            topic: "remember milk".to_string(),
        }
    }

    #[test]
    fn full_prefixless_and_bare_identifiers_resolve() {
        for identifier in [
            "PWF-NOTE-0007",
            concat!("pwf", "-note-0007"),
            "note-0007",
            "7",
        ] {
            let store = InMemoryStore::default().with_project_notes("pwf", vec![note()]);

            let removed = super::execute_for_project(
                RemoveNote {
                    project_identifier: "PWF".to_string(),
                    id: identifier.to_string(),
                },
                &store,
                &project("PWF", "pwf"),
            )
            .unwrap();

            assert_eq!(removed.id.as_ref(), "PWF-NOTE-0007");
            assert!(store.project_notes("pwf").is_empty());
        }
    }

    #[test]
    fn missing_note_wins_over_adapter_delete_failure() {
        let store = InMemoryStore::default().with_failure(ProjectNoteFailure::Delete);

        let error = super::execute_for_project(
            RemoveNote {
                project_identifier: "pwf".to_string(),
                id: "1".to_string(),
            },
            &store,
            &project("PWF", "pwf"),
        )
        .unwrap_err();

        assert!(matches!(
            error,
            RemoveNoteError::NoSuchNote {
                ref id,
                ref project,
            } if id == "PWF-NOTE-0001" && project == "pwf"
        ));
    }

    #[test]
    fn identifier_from_another_project_is_rejected() {
        let store = InMemoryStore::default();

        let error = super::execute_for_project(
            RemoveNote {
                project_identifier: "pwf".to_string(),
                id: "FOO-NOTE-0001".to_string(),
            },
            &store,
            &project("PWF", "pwf"),
        )
        .unwrap_err();

        assert!(matches!(
            error,
            RemoveNoteError::InvalidIdentifier {
                ref id,
                ref project_id,
            } if id == "FOO-NOTE-0001" && project_id == "PWF"
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
