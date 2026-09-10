use pwf_application::note::{list_notes, list_notes::ListNotesError};
use pwf_wire::note::{ListNotes, ListedNotes};

use crate::support::{InMemoryStore, insert_project, project_note};

#[derive(Debug, thiserror::Error)]
#[error("sentinel store failure")]
struct SentinelStoreError;

fn identifiers(result: &ListedNotes) -> Vec<&str> {
    result.notes.iter().map(|note| note.id.as_ref()).collect()
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn list_orders_newest_first_and_defaults_to_ten(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
    let store = InMemoryStore::default().with_project_notes(
        "foo",
        [1, 12, 5, 3, 11, 8, 2, 10, 7, 4, 9, 6]
            .into_iter()
            .map(|number| project_note(number, format!("note {number}")))
            .collect(),
    );

    let result = list_notes::execute(
        ListNotes {
            project_id: "FOO".parse().unwrap(),
            limit: None.into(),
        },
        &store,
        &pool,
    )
    .await
    .unwrap();

    assert_eq!(
        identifiers(&result),
        vec![
            "FOO-NOTE-0012",
            "FOO-NOTE-0011",
            "FOO-NOTE-0010",
            "FOO-NOTE-0009",
            "FOO-NOTE-0008",
            "FOO-NOTE-0007",
            "FOO-NOTE-0006",
            "FOO-NOTE-0005",
            "FOO-NOTE-0004",
            "FOO-NOTE-0003",
        ]
    );
    assert_eq!(result.hidden, 2);
    assert_eq!(result.project.as_ref(), "foo");
}

#[sqlx::test(migrator = "crate::support::MIGRATOR")]
async fn zero_is_unlimited_and_explicit_cap_reports_hidden_count(pool: sqlx::SqlitePool) {
    insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
    let store = InMemoryStore::default().with_project_notes(
        "foo",
        (1..=4)
            .map(|number| project_note(number, format!("note {number}")))
            .collect(),
    );

    let unlimited = list_notes::execute(
        ListNotes {
            project_id: "foo".parse().unwrap(),
            limit: Some(0).into(),
        },
        &store,
        &pool,
    )
    .await
    .unwrap();
    let capped = list_notes::execute(
        ListNotes {
            project_id: "foo".parse().unwrap(),
            limit: Some(2).into(),
        },
        &store,
        &pool,
    )
    .await
    .unwrap();

    assert_eq!(
        identifiers(&unlimited),
        vec![
            "FOO-NOTE-0004",
            "FOO-NOTE-0003",
            "FOO-NOTE-0002",
            "FOO-NOTE-0001",
        ]
    );
    assert_eq!(unlimited.hidden, 0);
    assert_eq!(identifiers(&capped), vec!["FOO-NOTE-0004", "FOO-NOTE-0003"]);
    assert_eq!(capped.hidden, 2);
}

#[test]
fn store_error_preserves_display_and_root_cause() {
    let error = ListNotesError::Store(anyhow::Error::new(SentinelStoreError));

    assert_eq!(error.to_string(), "sentinel store failure");
    let source = match error {
        ListNotesError::Store(source) => Some(source),
        ListNotesError::GetProject(_) => None,
    };
    assert!(source.is_some());
    let source = source.unwrap();
    assert!(source.downcast_ref::<SentinelStoreError>().is_some());
    assert_eq!(source.root_cause().to_string(), "sentinel store failure");
}
