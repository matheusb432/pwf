//! Removes one note from a managed project.

use pwf_domain::note::NoteId;

use super::identifier::{self, ResolvedProject};
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

/// Reports the canonical identifier of a removed note.
///
/// # Examples
///
/// ```
/// use pwf_application::note::remove_note::RemovedNote;
/// use pwf_domain::note::NoteId;
///
/// let removed = RemovedNote {
///     id: NoteId::try_new("PWF-NOTE-0001").unwrap(),
/// };
/// assert_eq!(removed.id.number(), 1);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovedNote {
    /// Identifies the removed note.
    pub id: NoteId,
}

/// Reports a rejected note-removal request or storage failure.
///
/// # Examples
///
/// ```
/// use pwf_application::note::remove_note::RemoveNoteError;
///
/// let error = RemoveNoteError::NoSuchNote {
///     id: "PWF-NOTE-0001".to_string(),
///     project: "pwf".to_string(),
/// };
/// assert_eq!(error.to_string(), "No such note PWF-NOTE-0001 in pwf.");
/// ```
#[derive(Debug, thiserror::Error)]
pub enum RemoveNoteError {
    /// Reports a project identifier that does not resolve to a managed project.
    #[error("Unknown project '{identifier}'; expected a managed project name or id code.")]
    UnknownProject {
        /// Preserves the unmatched project identifier.
        identifier: String,
    },
    /// Reports a note identifier that cannot resolve within the selected project.
    #[error("Invalid note id '{id}'; expected e.g. {prefix}-NOTE-0001, NOTE-0001, or 1.")]
    InvalidIdentifier {
        /// Preserves the rejected note identifier.
        id: String,
        /// Names the selected project's canonical prefix.
        prefix: String,
    },
    /// Reports a resolved identifier whose note representation does not exist.
    #[error("No such note {id} in {project}.")]
    NoSuchNote {
        /// Identifies the missing note canonically.
        id: String,
        /// Names the selected project.
        project: String,
    },
    /// Preserves the storage adapter failure as the causal source.
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
/// #     note::remove_note::{self, RemoveNote, RemoveNoteError, RemovedNote},
/// #     pending_work::ProjectRegistry,
/// # };
/// # fn remove<S>(
/// #     request: RemoveNote,
/// #     store: &S,
/// #     projects: &ProjectRegistry,
/// # ) -> Result<RemovedNote, RemoveNoteError>
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
) -> Result<RemovedNote, RemoveNoteError>
where
    S: ProjectNoteStore,
{
    let RemoveNote {
        project_identifier,
        id: raw_id,
    } = command;
    let ResolvedProject { project, prefix } =
        identifier::resolve_project(projects, &project_identifier).ok_or_else(|| {
            RemoveNoteError::UnknownProject {
                identifier: project_identifier,
            }
        })?;
    let id = identifier::resolve_note(&raw_id, &prefix).ok_or_else(|| {
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
    Ok(RemovedNote { id })
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use pwf_domain::{note::NoteId, pending_work::ProjectName};

    use super::{RemoveNote, RemoveNoteError};
    use crate::{
        ProjectNote,
        pending_work::ProjectRegistry,
        testing::{FailurePoint, InMemoryStore},
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
            message: "remember milk".to_string(),
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
        let store = InMemoryStore::default().with_failure(FailurePoint::ProjectNoteDelete);

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
