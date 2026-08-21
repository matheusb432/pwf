//! Lists one managed project's notes.

use crate::{
    contract::{
        note::{ListNotes, ListedNote, ListedNotes, NoteListLimit},
        project::{ProjectStatusFilter, ResolveProject},
    },
    ports::project_note::ProjectNoteStore,
    project::resolve_project::{self, ResolveProjectError},
};

const DEFAULT_NOTE_COUNT: usize = 10;

#[derive(Debug, thiserror::Error)]
pub enum ListNotesError {
    #[error(transparent)]
    ResolveProject(#[from] ResolveProjectError),
    #[error("{0}")]
    Store(#[source] Box<dyn std::error::Error + Send + Sync>),
}

/// Reads, orders, and caps one project's notes.
///
/// # Errors
///
/// Returns [`ListNotesError::ResolveProject`] when the project does not resolve or
/// [`ListNotesError::Store`] when listing notes fails.
#[cqrsy::query]
pub async fn execute(
    query: ListNotes,
    store: &impl ProjectNoteStore,
    pool: &sqlx::SqlitePool,
) -> Result<ListedNotes, ListNotesError> {
    let project = resolve_project::execute(
        ResolveProject {
            selector: query.project_selector,
            status: ProjectStatusFilter::ActiveOnly,
        },
        pool,
    )
    .await?;
    let mut notes = store
        .list_notes(&project)
        .map_err(|error| ListNotesError::Store(Box::new(error)))?;
    notes.sort_by_key(|note| std::cmp::Reverse(note.id.number()));
    let shown = shown_count(query.limit, notes.len());
    let hidden = notes.len() - shown;
    let notes = notes
        .into_iter()
        .take(shown)
        .map(|note| ListedNote {
            id: note.id,
            title: note.title,
        })
        .collect();
    Ok(ListedNotes {
        project: project.title.clone(),
        notes,
        hidden,
    })
}

fn shown_count(limit: NoteListLimit, available: usize) -> usize {
    match limit {
        NoteListLimit::Default => DEFAULT_NOTE_COUNT.min(available),
        NoteListLimit::Unlimited => available,
        NoteListLimit::AtMost(limit) => limit.get().min(available),
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use pwf_models::note::{NoteId, NoteTitle, ProjectNote};

    use super::{ListNotes, ListNotesError};
    use crate::{
        contract::note::ListedNotes,
        note::list_notes,
        testing::{InMemoryStore, insert_project},
    };

    #[derive(Debug, thiserror::Error)]
    #[error("sentinel store failure")]
    struct SentinelStoreError;

    fn note(number: u32) -> ProjectNote {
        ProjectNote {
            id: NoteId::try_new(format!("PWF-NOTE-{number:04}")).unwrap(),
            title: NoteTitle::try_new(format!("note {number}")).unwrap(),
        }
    }

    fn identifiers(result: &ListedNotes) -> Vec<&str> {
        result.notes.iter().map(|note| note.id.as_ref()).collect()
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn list_orders_newest_first_and_defaults_to_ten(pool: sqlx::SqlitePool) {
        insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
        let store = InMemoryStore::default().with_project_notes(
            "pwf",
            [1, 12, 5, 3, 11, 8, 2, 10, 7, 4, 9, 6]
                .into_iter()
                .map(note)
                .collect(),
        );

        let result = list_notes::execute(
            ListNotes {
                project_selector: "PWF".parse().unwrap(),
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
                "PWF-NOTE-0012",
                "PWF-NOTE-0011",
                "PWF-NOTE-0010",
                "PWF-NOTE-0009",
                "PWF-NOTE-0008",
                "PWF-NOTE-0007",
                "PWF-NOTE-0006",
                "PWF-NOTE-0005",
                "PWF-NOTE-0004",
                "PWF-NOTE-0003",
            ]
        );
        assert_eq!(result.hidden, 2);
        assert_eq!(result.project.as_ref(), "pwf");
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn zero_is_unlimited_and_explicit_cap_reports_hidden_count(pool: sqlx::SqlitePool) {
        insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
        let store = InMemoryStore::default().with_project_notes("pwf", (1..=4).map(note).collect());

        let unlimited = list_notes::execute(
            ListNotes {
                project_selector: "pwf".parse().unwrap(),
                limit: Some(0).into(),
            },
            &store,
            &pool,
        )
        .await
        .unwrap();
        let capped = list_notes::execute(
            ListNotes {
                project_selector: "pwf".parse().unwrap(),
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
                "PWF-NOTE-0004",
                "PWF-NOTE-0003",
                "PWF-NOTE-0002",
                "PWF-NOTE-0001",
            ]
        );
        assert_eq!(unlimited.hidden, 0);
        assert_eq!(identifiers(&capped), vec!["PWF-NOTE-0004", "PWF-NOTE-0003"]);
        assert_eq!(capped.hidden, 2);
    }

    #[test]
    fn store_error_preserves_display_and_source() {
        let error = ListNotesError::Store(Box::new(SentinelStoreError));

        assert_eq!(error.to_string(), "sentinel store failure");
        let source = error.source().expect("store error retains its source");
        assert!(source.downcast_ref::<SentinelStoreError>().is_some());
        assert_eq!(source.to_string(), "sentinel store failure");
    }
}
