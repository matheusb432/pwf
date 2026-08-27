//! Updates one note title in a managed project.

use pwf_models::{
    note::NoteSelector,
    project::{ProjectId, ProjectName},
};
use pwf_wire::{
    note::{NoteSummary, UpdateNote},
    project::{ProjectStatusFilter, ResolveProject},
};

use crate::{
    ports::project_note::{ProjectNotePatch, ProjectNoteStore},
    project::resolve_project::{self, ResolveProjectError},
};

#[derive(Debug, thiserror::Error)]
pub enum UpdateNoteError {
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

/// Resolves one note and replaces its title while preserving stored content and metadata.
#[cqrsy::command]
pub async fn execute(
    command: UpdateNote,
    store: &impl ProjectNoteStore,
    pool: &sqlx::SqlitePool,
) -> Result<NoteSummary, UpdateNoteError> {
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
            .ok_or_else(|| UpdateNoteError::ProjectMismatch {
                selector: command.selector,
                project_id: project.id.clone(),
            })?;
    let existing = store
        .get_note(&project, &id)
        .map_err(|error| UpdateNoteError::Store(anyhow::Error::new(error)))?;
    if existing.is_none() {
        return Err(UpdateNoteError::NoSuchNote {
            id,
            project: project.title,
        });
    }
    store
        .update_note(
            &project,
            &id,
            ProjectNotePatch {
                title: command.title.clone(),
            },
        )
        .map_err(|error| UpdateNoteError::Store(anyhow::Error::new(error)))?;
    Ok(NoteSummary {
        id,
        title: command.title,
    })
}

#[cfg(test)]
mod tests {
    use pwf_models::note::{NoteId, NoteTitle, ProjectNote};

    use super::{UpdateNote, UpdateNoteError};
    use crate::{
        note::update_note,
        testing::{InMemoryStore, insert_project},
    };

    #[derive(Debug, thiserror::Error)]
    #[error("sentinel store failure")]
    struct SentinelStoreError;

    fn note() -> ProjectNote {
        ProjectNote {
            id: NoteId::try_new("PWF-NOTE-0007").unwrap(),
            title: NoteTitle::try_new("old message").unwrap(),
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

            let updated = update_note::execute(
                UpdateNote {
                    project_selector: "pwf".parse().unwrap(),
                    selector: identifier.parse().unwrap(),
                    title: NoteTitle::try_new(" new message \t").unwrap(),
                },
                &store,
                &pool,
            )
            .await
            .unwrap();

            assert_eq!(updated.id.as_ref(), "PWF-NOTE-0007");
            assert_eq!(updated.title.as_ref(), "new message");
            assert_eq!(store.project_notes("pwf")[0].title.as_ref(), "new message");
            assert!(store.entries("pwf").is_empty());
        }
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn missing_note_is_reported(pool: sqlx::SqlitePool) {
        insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
        let store = InMemoryStore::default();

        let error = update_note::execute(
            UpdateNote {
                project_selector: "pwf".parse().unwrap(),
                selector: "note-0007".parse().unwrap(),
                title: NoteTitle::try_new("new message").unwrap(),
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
            } if id.as_ref() == "PWF-NOTE-0007" && project.as_ref() == "pwf"
        ));
        assert!(store.project_notes("pwf").is_empty());
    }

    #[test]
    fn store_error_preserves_display_and_root_cause() {
        let error = UpdateNoteError::Store(anyhow::Error::new(SentinelStoreError));

        assert_eq!(error.to_string(), "sentinel store failure");
        let UpdateNoteError::Store(source) = error else {
            panic!("expected the store error");
        };
        assert!(source.downcast_ref::<SentinelStoreError>().is_some());
        assert_eq!(source.root_cause().to_string(), "sentinel store failure");
    }
}
