use pwf_application::note::{add_note, add_note::AddNoteError};
use pwf_models::{
    AppDate,
    note::{NoteContent, NoteTitle},
};
use pwf_wire::note::AddNote;

use crate::support::{FixedClock, InMemoryStore, insert_project, project_note};

#[derive(Debug, thiserror::Error)]
#[error("sentinel store failure")]
struct SentinelStoreError;

fn command(project_id: &str) -> AddNote {
    AddNote {
        project_id: project_id.parse().unwrap(),
        title: NoteTitle::try_new(" remember milk ").unwrap(),
        content: NoteContent::try_new(" buy milk before the store closes ").unwrap(),
        domain: None,
        tags: Vec::new(),
        sources: Vec::new(),
        verified: None,
        date: Some("2026-07-15".parse().unwrap()),
    }
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn maximum_suffix_allocates_the_next_identifier(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
    for project_id in ["foo", "FOO"] {
        let store = InMemoryStore::default().with_project_notes(
            "foo",
            vec![
                project_note(2, "two"),
                project_note(9, "nine"),
                project_note(4, "four"),
            ],
        );

        let added = add_note::execute(command(project_id), &store, &pool, &FixedClock)
            .await
            .unwrap();

        assert_eq!(added.id.as_ref(), "FOO-NOTE-0010");
        assert_eq!(added.title.as_ref(), "remember milk");
        assert_eq!(
            store.project_notes("foo").last().unwrap().id.as_ref(),
            "FOO-NOTE-0010"
        );
    }
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn explicit_date_overrides_the_clock(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
    let store = InMemoryStore::default();

    add_note::execute(command("foo"), &store, &pool, &FixedClock)
        .await
        .unwrap();

    assert_eq!(
        store.project_note_creations("foo"),
        vec!["2026-07-15".parse::<AppDate>().unwrap()]
    );
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn absent_date_uses_the_clock_once(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
    let store = InMemoryStore::default();
    let mut command = command("foo");
    command.date = None;

    add_note::execute(command, &store, &pool, &FixedClock)
        .await
        .unwrap();

    assert_eq!(
        store.project_note_creations("foo"),
        vec!["2026-07-26".parse::<AppDate>().unwrap()]
    );
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn exhausted_four_digit_suffix_is_reported_without_inserting(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
    let store =
        InMemoryStore::default().with_project_notes("foo", vec![project_note(9_999, "last")]);

    let error = add_note::execute(command("foo"), &store, &pool, &FixedClock)
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        AddNoteError::IdentifierExhausted { ref project } if project.as_ref() == "foo"
    ));
    assert_eq!(
        store.project_notes("foo"),
        vec![project_note(9_999, "last")]
    );
}

#[test]
fn store_error_preserves_display_and_root_cause() {
    let error = AddNoteError::Store(anyhow::Error::new(SentinelStoreError));

    assert_eq!(error.to_string(), "sentinel store failure");
    let source = match error {
        AddNoteError::Store(source) => Some(source),
        _ => None,
    };
    assert!(source.is_some());
    let source = source.unwrap();
    assert!(source.downcast_ref::<SentinelStoreError>().is_some());
    assert_eq!(source.root_cause().to_string(), "sentinel store failure");
}
