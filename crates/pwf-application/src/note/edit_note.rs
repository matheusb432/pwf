//! Edits selected fields of one note in a managed project.

use pwf_models::{
    note::NoteSelector,
    project::{ProjectId, ProjectName},
};
use pwf_wire::note::{EditNote, MutatedNote};

use crate::{
    ports::project_note::{ProjectNotePatch, ProjectNotes},
    project::{get_active_project, get_project::GetProjectError},
};

#[derive(Debug, thiserror::Error)]
pub enum EditNoteError {
    #[error(transparent)]
    GetProject(#[from] GetProjectError),
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

/// Resolves one note and applies only the explicitly selected changes.
#[cqrsy::command]
pub async fn execute(
    command: EditNote,
    store: &impl ProjectNotes,
    pool: &sqlx::SqlitePool,
) -> Result<MutatedNote, EditNoteError> {
    let project = get_active_project::execute(command.project_id, pool).await?;
    let id =
        command
            .selector
            .resolve(&project.id)
            .ok_or_else(|| EditNoteError::ProjectMismatch {
                selector: command.selector,
                project_id: project.id.clone(),
            })?;
    let existing = store
        .get_note(&project, &id)
        .map_err(|error| EditNoteError::Store(anyhow::Error::new(error)))?
        .ok_or_else(|| EditNoteError::NoSuchNote {
            id: id.clone(),
            project: project.title.clone(),
        })?;
    let patch = ProjectNotePatch {
        title: command.edits.title().clone(),
        content: command.edits.content().clone(),
        domain: command.edits.domain().clone(),
        tags: command.edits.tags().clone(),
        sources: command.edits.sources().clone(),
        verified: command.edits.verified().clone(),
    };
    let mut title = existing.title;
    patch.title.clone().apply(&mut title);
    store
        .update_note(&project, &id, patch)
        .map_err(|error| EditNoteError::Store(anyhow::Error::new(error)))?;
    Ok(MutatedNote {
        id,
        project: project.title,
        title,
    })
}

#[cfg(test)]
mod tests {
    use pwf_models::note::{
        NoteContent, NoteDomain, NoteSource, NoteTag, NoteTitle, NoteVerification,
    };
    use pwf_wire::{
        collection_edit::CollectionEdit,
        note::{EditNote, NoteEdits},
        patch_field::PatchField,
        set_field::SetField,
    };

    use super::EditNoteError;
    use crate::{
        note::edit_note,
        ports::project_note::ProjectNotePatch,
        testing::{InMemoryStore, insert_project, project_note},
    };

    fn edits(title: SetField<&str>) -> NoteEdits {
        NoteEdits::try_new(
            title.map(|value| NoteTitle::try_new(value).unwrap()),
            SetField::Set(NoteContent::try_new("new content").unwrap()),
            PatchField::Clear,
            CollectionEdit::Append(vec![NoteTag::try_new("new-tag").unwrap()]),
            CollectionEdit::Replace(vec![NoteSource::try_new("new source").unwrap()]),
            PatchField::Set(NoteVerification::try_new("2026-08-30").unwrap()),
        )
        .unwrap()
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn explicit_edits_preserve_omitted_title_and_pass_each_patch_operation(
        pool: sqlx::SqlitePool,
    ) {
        insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
        let store = InMemoryStore::default()
            .with_project_notes("foo", vec![project_note(7, "old message")]);

        let edited = edit_note::execute(
            EditNote {
                project_id: "foo".parse().unwrap(),
                selector: "7".parse().unwrap(),
                edits: edits(SetField::NoAction),
            },
            &store,
            &pool,
        )
        .await
        .unwrap();

        assert_eq!(edited.id.as_ref(), "FOO-NOTE-0007");
        assert_eq!(edited.project.as_ref(), "foo");
        assert_eq!(edited.title.as_ref(), "old message");
        assert_eq!(
            store.project_note_patches("foo"),
            vec![ProjectNotePatch {
                title: SetField::NoAction,
                content: SetField::Set(NoteContent::try_new("new content").unwrap()),
                domain: PatchField::<NoteDomain>::Clear,
                tags: CollectionEdit::Append(vec![NoteTag::try_new("new-tag").unwrap()]),
                sources: CollectionEdit::Replace(vec![NoteSource::try_new("new source").unwrap(),]),
                verified: PatchField::Set(NoteVerification::try_new("2026-08-30").unwrap(),),
            }]
        );
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn full_prefixless_and_bare_identifiers_resolve(pool: sqlx::SqlitePool) {
        insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
        for identifier in ["FOO-NOTE-0007", "foo-note-0007", "note-0007", "7"] {
            let store = InMemoryStore::default()
                .with_project_notes("foo", vec![project_note(7, "old message")]);

            let edited = edit_note::execute(
                EditNote {
                    project_id: "foo".parse().unwrap(),
                    selector: identifier.parse().unwrap(),
                    edits: edits(SetField::Set("new message")),
                },
                &store,
                &pool,
            )
            .await
            .unwrap();

            assert_eq!(edited.title.as_ref(), "new message");
            assert_eq!(store.project_notes("foo")[0].title.as_ref(), "new message");
        }
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn missing_note_is_reported(pool: sqlx::SqlitePool) {
        insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
        let store = InMemoryStore::default();

        let error = edit_note::execute(
            EditNote {
                project_id: "foo".parse().unwrap(),
                selector: "7".parse().unwrap(),
                edits: edits(SetField::Set("new message")),
            },
            &store,
            &pool,
        )
        .await
        .unwrap_err();

        assert!(matches!(
            error,
            EditNoteError::NoSuchNote { ref id, ref project }
                if id.as_ref() == "FOO-NOTE-0007" && project.as_ref() == "foo"
        ));
    }
}
