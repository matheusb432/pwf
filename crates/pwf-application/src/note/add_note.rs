//! Adds one note to a managed project.

use pwf_models::{note::NoteId, project::ProjectName, task::TaskTimestampError};
use pwf_wire::{
    note::{AddNote, NoteSummary},
    project::{ProjectStatusFilter, ResolveProject},
};

use crate::{
    ports::{
        clock::Clock,
        project_note::{NewProjectNote, ProjectNoteStore},
    },
    project::resolve_project::{self, ResolveProjectError},
};

#[derive(Debug, thiserror::Error)]
pub enum AddNoteError {
    #[error(transparent)]
    ResolveProject(#[from] ResolveProjectError),
    #[error("Project '{project}' has no available four-digit note identifiers.")]
    IdentifierExhausted { project: ProjectName },
    #[error(transparent)]
    Store(anyhow::Error),
    #[error("cannot read the current date: {0}")]
    Clock(#[from] TaskTimestampError),
}

/// Validates, allocates, and persists one project note.
#[cqrsy::command]
pub async fn execute(
    command: AddNote,
    store: &impl ProjectNoteStore,
    pool: &sqlx::SqlitePool,
    clock: &impl Clock,
) -> Result<NoteSummary, AddNoteError> {
    let project = resolve_project::execute(
        ResolveProject {
            selector: command.project_selector,
            status: ProjectStatusFilter::ActiveOnly,
        },
        pool,
    )
    .await?;
    let notes = store
        .list_notes(&project)
        .map_err(|error| AddNoteError::Store(anyhow::Error::new(error)))?;
    let next_number = notes
        .iter()
        .map(|note| note.id.number())
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .filter(|number| *number <= 9_999)
        .ok_or_else(|| AddNoteError::IdentifierExhausted {
            project: project.title.clone(),
        })?;
    let id = NoteId::try_new(format!("{}-NOTE-{next_number:04}", project.id)).map_err(|_| {
        AddNoteError::IdentifierExhausted {
            project: project.title.clone(),
        }
    })?;
    let created = match command.date {
        Some(date) => date,
        None => clock.today()?,
    };
    let created = store
        .insert_note(
            &project,
            NewProjectNote {
                id,
                title: command.title,
                content: command.content,
                why: command.why,
                domain: command.domain,
                tags: command.tags,
                sources: command.sources,
                verified: command.verified,
                created,
            },
        )
        .map_err(|error| AddNoteError::Store(anyhow::Error::new(error)))?;
    Ok(NoteSummary {
        id: created.id,
        title: created.title,
    })
}

#[cfg(test)]
mod tests {
    use pwf_models::{
        AppDate,
        note::{NoteContent, NoteId, NoteTitle, ProjectNote},
    };

    use super::{AddNote, AddNoteError};
    use crate::{
        note::add_note,
        testing::{FixedClock, InMemoryStore, insert_project},
    };

    #[derive(Debug, thiserror::Error)]
    #[error("sentinel store failure")]
    struct SentinelStoreError;

    fn note(number: u32, title: &str) -> ProjectNote {
        ProjectNote {
            id: NoteId::try_new(format!("FOO-NOTE-{number:04}")).unwrap(),
            title: NoteTitle::try_new(title).unwrap(),
        }
    }

    fn command(project_selector: &str) -> AddNote {
        AddNote {
            project_selector: project_selector.parse().unwrap(),
            title: NoteTitle::try_new(" remember milk ").unwrap(),
            content: NoteContent::try_new(" buy milk before the store closes ").unwrap(),
            why: None,
            domain: None,
            tags: Vec::new(),
            sources: Vec::new(),
            verified: None,
            date: Some("2026-07-15".parse().unwrap()),
        }
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn maximum_suffix_allocates_the_next_identifier(pool: sqlx::SqlitePool) {
        insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
        for project_selector in ["foo", "FOO"] {
            let store = InMemoryStore::default().with_project_notes(
                "foo",
                vec![note(2, "two"), note(9, "nine"), note(4, "four")],
            );

            let added = add_note::execute(command(project_selector), &store, &pool, &FixedClock)
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

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
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

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
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

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn exhausted_four_digit_suffix_is_reported_without_inserting(pool: sqlx::SqlitePool) {
        insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
        let store = InMemoryStore::default().with_project_notes("foo", vec![note(9_999, "last")]);

        let error = add_note::execute(command("foo"), &store, &pool, &FixedClock)
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            AddNoteError::IdentifierExhausted { ref project } if project.as_ref() == "foo"
        ));
        assert_eq!(store.project_notes("foo"), vec![note(9_999, "last")]);
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
}
