use pwf_application::{
    note::{edit_note, edit_note::EditNoteError},
    ports::project_note::ProjectNotePatch,
};
use pwf_models::note::{NoteContent, NoteDomain, NoteSource, NoteTag, NoteTitle, NoteVerification};
use pwf_wire::{
    collection_edit::CollectionEdit,
    note::{EditNote, NoteEdits},
    patch_field::PatchField,
    set_field::SetField,
};

use crate::support::{InMemoryStore, insert_project, project_note};

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

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn explicit_edits_preserve_omitted_title_and_pass_each_patch_operation(
    pool: sqlx::SqlitePool,
) {
    insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
    let store =
        InMemoryStore::default().with_project_notes("foo", vec![project_note(7, "old message")]);

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

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
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

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
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
