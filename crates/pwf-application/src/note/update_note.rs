//! Updates one note title in a managed project.

use pwf_models::{
    note::NoteId,
    project::{ProjectId, ProjectSelector},
};
use pwf_wire::project::ProjectStatusFilter;

use super::resolve_note;
use crate::{
    ports::project_note::{ProjectNotePatch, ProjectNoteStore},
    project::resolve_project::{self, ResolveProject, ResolveProjectError},
};

/// Requests replacement of one project note's title.
#[derive(Debug, Clone)]
pub struct UpdateNote {
    /// Selects the managed project by name or id code.
    pub project_selector: ProjectSelector,
    /// Selects the note by full id, `NOTE-NNNN`, or bare numeric suffix.
    pub id: String,
    /// Supplies the replacement title.
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateNoteOk {
    pub id: NoteId,
    pub title: String,
}

#[derive(Debug, thiserror::Error)]
pub enum UpdateNoteError {
    #[error("Unknown project '{selector}'; expected a managed project name or id code.")]
    UnknownProject { selector: ProjectSelector },
    #[error("Note title is empty; provide a non-empty title.")]
    EmptyTitle,
    #[error("Invalid note id '{id}'; expected e.g. {project_id}-NOTE-0001, NOTE-0001, or 1.")]
    InvalidIdentifier { id: String, project_id: ProjectId },
    #[error("No such note {id} in {project}.")]
    NoSuchNote { id: String, project: String },
    #[error("{0}")]
    Project(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("{0}")]
    Store(#[source] Box<dyn std::error::Error + Send + Sync>),
}

/// Resolves one note and replaces its title while preserving stored content and metadata.
///
/// # Errors
///
/// Returns [`UpdateNoteError::UnknownProject`] when the project does not resolve,
/// [`UpdateNoteError::EmptyTitle`] when the normalized replacement is empty,
/// [`UpdateNoteError::InvalidIdentifier`] when the note id is invalid for that project,
/// [`UpdateNoteError::NoSuchNote`] when the note does not exist, or
/// [`UpdateNoteError::Store`] when reading or updating the note fails.
#[cqrsy::command]
pub async fn execute(
    command: UpdateNote,
    store: &impl ProjectNoteStore,
    pool: &sqlx::SqlitePool,
) -> Result<UpdateNoteOk, UpdateNoteError> {
    let project = resolve_project::execute(
        ResolveProject {
            selector: command.project_selector,
            status: ProjectStatusFilter::ACTIVE,
        },
        pool,
    )
    .await
    .map_err(project_error)?;
    let title = command
        .title
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if title.is_empty() {
        return Err(UpdateNoteError::EmptyTitle);
    }
    let id = resolve_note(&command.id, &project.id).ok_or_else(|| {
        UpdateNoteError::InvalidIdentifier {
            id: command.id,
            project_id: project.id.clone(),
        }
    })?;
    let existing = store
        .get_note(&project, &id)
        .map_err(|error| UpdateNoteError::Store(Box::new(error)))?;
    if existing.is_none() {
        return Err(UpdateNoteError::NoSuchNote {
            id: id.to_string(),
            project: project.title.to_string(),
        });
    }
    store
        .update_note(
            &project,
            &id,
            ProjectNotePatch {
                title: title.clone(),
            },
        )
        .map_err(|error| UpdateNoteError::Store(Box::new(error)))?;
    Ok(UpdateNoteOk { id, title })
}

fn project_error(error: ResolveProjectError) -> UpdateNoteError {
    match error {
        ResolveProjectError::Unknown { selector, .. } => {
            UpdateNoteError::UnknownProject { selector }
        }
        error @ ResolveProjectError::Unexpected { .. } => UpdateNoteError::Project(Box::new(error)),
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use pwf_models::note::{NoteId, ProjectNote};

    use super::{UpdateNote, UpdateNoteError};
    use crate::testing::{InMemoryStore, insert_project};

    #[derive(Debug, thiserror::Error)]
    #[error("sentinel store failure")]
    struct SentinelStoreError;

    fn note() -> ProjectNote {
        ProjectNote {
            id: NoteId::try_new("PWF-NOTE-0007").unwrap(),
            title: "old message".to_string(),
        }
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn full_prefixless_and_bare_identifiers_resolve_and_trim_the_replacement(
        pool: sqlx::SqlitePool,
    ) {
        insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
        for identifier in [
            "PWF-NOTE-0007",
            concat!("pwf", "-note-0007"),
            "note-0007",
            "7",
        ] {
            let store = InMemoryStore::default().with_project_notes("pwf", vec![note()]);

            let updated = super::execute(
                UpdateNote {
                    project_selector: "pwf".parse().unwrap(),
                    id: identifier.to_string(),
                    title: " new message \t".to_string(),
                },
                &store,
                &pool,
            )
            .await
            .unwrap();

            assert_eq!(updated.id.as_ref(), "PWF-NOTE-0007");
            assert_eq!(updated.title, "new message");
            assert_eq!(store.project_notes("pwf")[0].title, "new message");
            assert!(store.entries("pwf").is_empty());
        }
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn blank_replacement_leaves_the_existing_note_unchanged(pool: sqlx::SqlitePool) {
        insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
        let store = InMemoryStore::default().with_project_notes("pwf", vec![note()]);

        let error = super::execute(
            UpdateNote {
                project_selector: "pwf".parse().unwrap(),
                id: "7".to_string(),
                title: " \t ".to_string(),
            },
            &store,
            &pool,
        )
        .await
        .unwrap_err();

        assert!(matches!(error, UpdateNoteError::EmptyTitle));
        assert_eq!(store.project_notes("pwf"), vec![note()]);
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn missing_note_is_reported(pool: sqlx::SqlitePool) {
        insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
        let store = InMemoryStore::default();

        let error = super::execute(
            UpdateNote {
                project_selector: "pwf".parse().unwrap(),
                id: "note-0007".to_string(),
                title: "new message".to_string(),
            },
            &store,
            &pool,
        )
        .await
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
