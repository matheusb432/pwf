//! Removes one note from a managed project.

use pwf_models::note::NoteId;

use super::logic::{self, ResolvedProject};
use crate::{ProjectNoteStore, pending_work::ProjectRegistry};

/// Requests deletion of one project note.
///
/// # Examples
///
/// ```
/// use pwf_application::note::remove_note::RemoveNote;
///
/// let request = RemoveNote {
///     project_identifier: "pwf".to_string(),
///     id: "NOTE-0001".to_string(),
/// };
/// assert_eq!(request.id, "NOTE-0001");
/// ```
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
    #[error("Invalid note id '{id}'; expected e.g. {prefix}-NOTE-0001, NOTE-0001, or 1.")]
    InvalidIdentifier { id: String, prefix: String },
    #[error("No such note {id} in {project}.")]
    NoSuchNote { id: String, project: String },
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
///
/// # Examples
///
/// ```
/// # use pwf_application::{
/// #     ProjectNoteStore,
/// #     note::remove_note::{self, RemoveNote, RemoveNoteError, RemoveNoteOk},
/// #     pending_work::ProjectRegistry,
/// # };
/// # fn remove<S>(
/// #     request: RemoveNote,
/// #     store: &S,
/// #     projects: &ProjectRegistry,
/// # ) -> Result<RemoveNoteOk, RemoveNoteError>
/// # where
/// #     S: ProjectNoteStore,
/// # {
/// remove_note::execute(request, store, projects)
/// # }
/// ```
#[cqrsy::command]
pub fn execute<S>(
    command: RemoveNote,
    store: &S,
    projects: &ProjectRegistry,
) -> Result<RemoveNoteOk, RemoveNoteError>
where
    S: ProjectNoteStore,
{
    let RemoveNote {
        project_identifier,
        id: raw_id,
    } = command;
    let ResolvedProject { project, prefix } = logic::resolve_project(projects, &project_identifier)
        .ok_or_else(|| RemoveNoteError::UnknownProject {
            identifier: project_identifier,
        })?;
    let id = logic::resolve_note(&raw_id, &prefix).ok_or_else(|| {
        RemoveNoteError::InvalidIdentifier {
            id: raw_id,
            prefix: prefix.to_string(),
        }
    })?;
    let exists = store
        .note_exists(&project, &id)
        .map_err(|error| RemoveNoteError::Store(Box::new(error)))?;
    if !exists {
        return Err(RemoveNoteError::NoSuchNote {
            id: id.to_string(),
            project: project.to_string(),
        });
    }
    store
        .delete(&project, &id)
        .map_err(|error| RemoveNoteError::Store(Box::new(error)))?;
    Ok(RemoveNoteOk { id })
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use pwf_models::{note::NoteId, pending_work::ProjectName};

    use super::{RemoveNote, RemoveNoteError};
    use crate::{
        ProjectNote,
        pending_work::ProjectRegistry,
        testing::{InMemoryStore, ProjectNoteFailure},
    };

    #[derive(Debug, thiserror::Error)]
    #[error("sentinel store failure")]
    struct SentinelStoreError;

    fn registry() -> ProjectRegistry {
        ProjectRegistry::new([(
            ProjectName::try_new("pwf").unwrap(),
            Some("/repo/pwf".to_string()),
            Some("PWF".to_string()),
        )])
    }

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

            let removed = super::execute(
                RemoveNote {
                    project_identifier: "PWF".to_string(),
                    id: identifier.to_string(),
                },
                &store,
                &registry(),
            )
            .unwrap();

            assert_eq!(removed.id.as_ref(), "PWF-NOTE-0007");
            assert!(store.project_notes("pwf").is_empty());
        }
    }

    #[test]
    fn missing_note_wins_over_adapter_delete_failure() {
        let store = InMemoryStore::default().with_failure(ProjectNoteFailure::Delete);

        let error = super::execute(
            RemoveNote {
                project_identifier: "pwf".to_string(),
                id: "1".to_string(),
            },
            &store,
            &registry(),
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

        let error = super::execute(
            RemoveNote {
                project_identifier: "pwf".to_string(),
                id: "FOO-NOTE-0001".to_string(),
            },
            &store,
            &registry(),
        )
        .unwrap_err();

        assert!(matches!(
            error,
            RemoveNoteError::InvalidIdentifier {
                ref id,
                ref prefix,
            } if id == "FOO-NOTE-0001" && prefix == "PWF"
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
