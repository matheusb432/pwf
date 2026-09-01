//! Removes one note from a managed project.

use pwf_models::{
    note::NoteSelector,
    project::{ProjectId, ProjectName},
};
use pwf_wire::{
    confirmation::RemoveNoteConfirmation,
    note::{MutatedNote, RemoveNote, RemovedNoteOutcome},
    project::{ProjectStatusFilter, ResolveProject},
};

use crate::{
    ports::{
        confirmation::{ConfirmationClient, ConfirmationClientError},
        project_note::ProjectNotes,
    },
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
    #[error(transparent)]
    Confirmation(#[from] ConfirmationClientError),
}

/// Resolves and deletes one note after confirming its representation exists.
#[cqrsy::command]
pub async fn execute(
    command: RemoveNote,
    store: &impl ProjectNotes,
    pool: &sqlx::SqlitePool,
    confirmation_client: &mut dyn ConfirmationClient<Confirmation = RemoveNoteConfirmation>,
) -> Result<RemovedNoteOutcome, RemoveNoteError> {
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
    let note = store
        .get_note(&project, &id)
        .map_err(|error| RemoveNoteError::Store(anyhow::Error::new(error)))?;
    let Some(note) = note else {
        return Err(RemoveNoteError::NoSuchNote {
            id,
            project: project.title,
        });
    };
    let confirmation = RemoveNoteConfirmation {
        note_identifier: id.clone(),
        project: project.title.clone(),
        title: note.title.clone(),
    };
    if !confirmation_client.confirm(&confirmation).await? {
        return Ok(RemovedNoteOutcome::Aborted { note_id: id });
    }
    store
        .delete_note(&project, &id)
        .map_err(|error| RemoveNoteError::Store(anyhow::Error::new(error)))?;
    Ok(RemovedNoteOutcome::Removed(MutatedNote {
        id,
        project: project.title,
        title: note.title,
    }))
}

#[cfg(test)]
mod tests {
    use futures::future::BoxFuture;
    use pwf_models::{
        note::{NoteId, NoteTitle, ProjectNote},
        project::{ProjectId, ProjectName},
    };
    use pwf_wire::{
        confirmation::RemoveNoteConfirmation,
        note::{RemoveNote, RemovedNoteOutcome},
    };

    use super::RemoveNoteError;
    use crate::{
        note::remove_note,
        ports::confirmation::{ConfirmationClient, ConfirmationClientError},
        testing::{InMemoryStore, InMemoryStoreFailure, insert_project},
    };

    #[derive(Debug, thiserror::Error)]
    #[error("sentinel store failure")]
    struct SentinelStoreError;

    struct TestConfirmation {
        accepted: bool,
        recorded: Vec<RemoveNoteConfirmation>,
    }

    impl TestConfirmation {
        fn accepting() -> Self {
            Self {
                accepted: true,
                recorded: Vec::new(),
            }
        }

        fn declining() -> Self {
            Self {
                accepted: false,
                recorded: Vec::new(),
            }
        }
    }

    impl ConfirmationClient for TestConfirmation {
        type Confirmation = RemoveNoteConfirmation;

        fn confirm<'a>(
            &'a mut self,
            confirmation: &'a RemoveNoteConfirmation,
        ) -> BoxFuture<'a, Result<bool, ConfirmationClientError>> {
            self.recorded.push(confirmation.clone());
            let accepted = self.accepted;
            Box::pin(std::future::ready(Ok(accepted)))
        }
    }

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
            let mut confirmation = TestConfirmation::accepting();

            let removed = remove_note::execute(
                RemoveNote {
                    project_selector: "FOO".parse().unwrap(),
                    selector: identifier.parse().unwrap(),
                },
                &store,
                &pool,
                &mut confirmation,
            )
            .await
            .unwrap();

            assert!(matches!(
                removed,
                RemovedNoteOutcome::Removed(ref note) if note.id.as_ref() == "FOO-NOTE-0007"
            ));
            assert_eq!(
                confirmation.recorded,
                vec![RemoveNoteConfirmation {
                    note_identifier: NoteId::try_new("FOO-NOTE-0007").unwrap(),
                    project: ProjectName::try_new("foo").unwrap(),
                    title: NoteTitle::try_new("remember milk").unwrap(),
                }]
            );
            assert!(store.project_notes("foo").is_empty());
        }
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn missing_note_wins_over_adapter_delete_failure(pool: sqlx::SqlitePool) {
        insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
        let store = InMemoryStore::default().with_failure(InMemoryStoreFailure::DeleteProjectNote);
        let mut confirmation = TestConfirmation::accepting();

        let error = remove_note::execute(
            RemoveNote {
                project_selector: "foo".parse().unwrap(),
                selector: "1".parse().unwrap(),
            },
            &store,
            &pool,
            &mut confirmation,
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
        let mut confirmation = TestConfirmation::accepting();

        let error = remove_note::execute(
            RemoveNote {
                project_selector: "foo".parse().unwrap(),
                selector: "BAR-NOTE-0001".parse().unwrap(),
            },
            &store,
            &pool,
            &mut confirmation,
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

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn decline_preserves_the_note(pool: sqlx::SqlitePool) {
        insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
        let store = InMemoryStore::default().with_project_notes("foo", vec![note()]);
        let mut confirmation = TestConfirmation::declining();

        let outcome = remove_note::execute(
            RemoveNote {
                project_selector: "foo".parse().unwrap(),
                selector: "7".parse().unwrap(),
            },
            &store,
            &pool,
            &mut confirmation,
        )
        .await
        .unwrap();

        assert!(matches!(outcome, RemovedNoteOutcome::Aborted { .. }));
        assert_eq!(store.project_notes("foo"), vec![note()]);
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
