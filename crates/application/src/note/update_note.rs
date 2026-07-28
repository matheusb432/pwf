//! Updates one note in a managed project.

use pwf_domain::note::NoteId;

use super::identifier::{self, ResolvedProject};
use crate::{AppRecordStore, ProjectNote, ProjectNotePatch, pending_work::ProjectRegistry};

/// Requests replacement of one project note's message.
///
/// # Examples
///
/// ```
/// use pwf_application::note::update_note::UpdateNote;
///
/// let request = UpdateNote {
///     project_identifier: "pwf".to_string(),
///     id: "1".to_string(),
///     message: "remember oat milk".to_string(),
/// };
/// assert_eq!(request.message, "remember oat milk");
/// ```
#[derive(Debug, Clone)]
pub struct UpdateNote {
    /// Selects the managed project by name or id code.
    pub project_identifier: String,
    /// Selects the note by full id, `NOTE-NNNN`, or bare numeric suffix.
    pub id: String,
    /// Supplies the replacement one-line message.
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateNoteOk {
    pub id: NoteId,
    pub message: String,
}

#[derive(Debug, thiserror::Error)]
pub enum UpdateNoteError {
    #[error("Unknown project '{identifier}'; expected a managed project name or id code.")]
    UnknownProject { identifier: String },
    #[error("Note message is empty; provide a non-empty message.")]
    EmptyMessage,
    #[error("Invalid note id '{id}'; expected e.g. {prefix}-NOTE-0001, NOTE-0001, or 1.")]
    InvalidIdentifier { id: String, prefix: String },
    #[error("No such note {id} in {project}.")]
    NoSuchNote { id: String, project: String },
    #[error("{0}")]
    Store(#[source] Box<dyn std::error::Error + Send + Sync>),
}

/// Resolves one note and replaces its message while preserving stored metadata.
///
/// # Errors
///
/// Returns [`UpdateNoteError::UnknownProject`] when the project does not resolve,
/// [`UpdateNoteError::EmptyMessage`] when the trimmed replacement is empty,
/// [`UpdateNoteError::InvalidIdentifier`] when the note id is invalid for that project,
/// [`UpdateNoteError::NoSuchNote`] when the note does not exist, or
/// [`UpdateNoteError::Store`] when reading or updating the note fails.
///
/// # Examples
///
/// ```
/// # use pwf_application::{
/// #     AppRecordStore, ProjectNote,
/// #     note::update_note::{self, UpdateNote, UpdateNoteError, UpdateNoteOk},
/// #     pending_work::ProjectRegistry,
/// # };
/// # fn update<S>(
/// #     request: UpdateNote,
/// #     store: &S,
/// #     projects: &ProjectRegistry,
/// # ) -> Result<UpdateNoteOk, UpdateNoteError>
/// # where
/// #     S: AppRecordStore<ProjectNote>,
/// # {
/// update_note::execute(request, store, projects)
/// # }
/// ```
#[cqrsy::command]
pub fn execute<S>(
    command: UpdateNote,
    store: &S,
    projects: &ProjectRegistry,
) -> Result<UpdateNoteOk, UpdateNoteError>
where
    S: AppRecordStore<ProjectNote>,
{
    let UpdateNote {
        project_identifier,
        id: raw_id,
        message,
    } = command;
    let ResolvedProject { project, prefix } =
        identifier::resolve_project(projects, &project_identifier).ok_or_else(|| {
            UpdateNoteError::UnknownProject {
                identifier: project_identifier,
            }
        })?;
    let message = message.trim();
    if message.is_empty() {
        return Err(UpdateNoteError::EmptyMessage);
    }
    let id = identifier::resolve_note(&raw_id, &prefix).ok_or_else(|| {
        UpdateNoteError::InvalidIdentifier {
            id: raw_id,
            prefix: prefix.to_string(),
        }
    })?;
    let existing = store
        .get(&project, &id)
        .map_err(|error| UpdateNoteError::Store(Box::new(error)))?;
    if existing.is_none() {
        return Err(UpdateNoteError::NoSuchNote {
            id: id.to_string(),
            project: project.to_string(),
        });
    }
    store
        .update(
            &project,
            &id,
            ProjectNotePatch {
                message: message.to_string(),
            },
        )
        .map_err(|error| UpdateNoteError::Store(Box::new(error)))?;
    Ok(UpdateNoteOk {
        id,
        message: message.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use pwf_domain::{note::NoteId, pending_work::ProjectName};

    use super::{UpdateNote, UpdateNoteError};
    use crate::{ProjectNote, pending_work::ProjectRegistry, testing::InMemoryStore};

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
            message: "old message".to_string(),
        }
    }

    #[test]
    fn full_prefixless_and_bare_identifiers_resolve_and_trim_the_replacement() {
        for identifier in [
            "PWF-NOTE-0007",
            concat!("pwf", "-note-0007"),
            "note-0007",
            "7",
        ] {
            let store = InMemoryStore::default().with_project_notes("pwf", vec![note()]);

            let updated = super::execute(
                UpdateNote {
                    project_identifier: "pwf".to_string(),
                    id: identifier.to_string(),
                    message: " new message \t".to_string(),
                },
                &store,
                &registry(),
            )
            .unwrap();

            assert_eq!(updated.id.as_ref(), "PWF-NOTE-0007");
            assert_eq!(updated.message, "new message");
            assert_eq!(store.project_notes("pwf")[0].message, "new message");
            assert!(store.entries("pwf").is_empty());
        }
    }

    #[test]
    fn blank_replacement_leaves_the_existing_note_unchanged() {
        let store = InMemoryStore::default().with_project_notes("pwf", vec![note()]);

        let error = super::execute(
            UpdateNote {
                project_identifier: "pwf".to_string(),
                id: "7".to_string(),
                message: " \t ".to_string(),
            },
            &store,
            &registry(),
        )
        .unwrap_err();

        assert!(matches!(error, UpdateNoteError::EmptyMessage));
        assert_eq!(store.project_notes("pwf"), vec![note()]);
    }

    #[test]
    fn missing_note_is_reported() {
        let store = InMemoryStore::default();

        let error = super::execute(
            UpdateNote {
                project_identifier: "pwf".to_string(),
                id: "note-0007".to_string(),
                message: "new message".to_string(),
            },
            &store,
            &registry(),
        )
        .unwrap_err();

        assert!(matches!(
            error,
            UpdateNoteError::NoSuchNote {
                ref id,
                ref project,
            } if id == "PWF-NOTE-0007" && project == "pwf"
        ));
        assert!(store.project_notes("pwf").is_empty());
    }

    #[test]
    fn store_error_preserves_display_and_source() {
        let error = UpdateNoteError::Store(Box::new(SentinelStoreError));

        assert_eq!(error.to_string(), "sentinel store failure");
        let source = error.source().expect("store error retains its source");
        assert!(source.downcast_ref::<SentinelStoreError>().is_some());
        assert_eq!(source.to_string(), "sentinel store failure");
    }
}
