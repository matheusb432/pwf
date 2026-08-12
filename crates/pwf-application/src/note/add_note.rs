//! Adds one note to a managed project.

use pwf_models::{
    AppDate,
    note::{
        NoteContent, NoteDomain, NoteId, NoteSource, NoteTag, NoteTitle, NoteVerification, NoteWhy,
    },
    project::{ProjectName, ProjectSelector},
};
use pwf_wire::{note::AddedNote, project::ProjectStatusFilter};

use crate::{
    ports::{
        clock::Clock,
        project_note::{NewProjectNote, ProjectNoteStore},
    },
    project::resolve_project::{self, ResolveProject, ResolveProjectError},
};

/// Requests creation of one project note.
#[derive(Debug, Clone)]
pub struct AddNote {
    /// Project's name or id
    pub project_selector: ProjectSelector,
    /// Names the note.
    pub title: NoteTitle,
    /// Supplies the note's Markdown body.
    pub content: NoteContent,
    /// Explains the consequence when it adds useful context.
    pub why: Option<NoteWhy>,
    /// Classifies the subject when known.
    pub domain: Option<NoteDomain>,
    /// Supplies discovery labels.
    pub tags: Vec<NoteTag>,
    /// Records supporting evidence.
    pub sources: Vec<NoteSource>,
    /// Records the supplied verification marker.
    pub verified: Option<NoteVerification>,
    /// Overrides the clock date when present.
    pub date: Option<AppDate>,
}

#[derive(Debug, thiserror::Error)]
pub enum AddNoteError {
    #[error(transparent)]
    ResolveProject(#[from] ResolveProjectError),
    #[error("Project '{project}' has no available four-digit note identifiers.")]
    IdentifierExhausted { project: ProjectName },
    #[error("{0}")]
    Store(#[source] Box<dyn std::error::Error + Send + Sync>),
}

/// Validates, allocates, and persists one project note.
///
/// # Errors
///
/// Returns [`AddNoteError::ResolveProject`] when the project does not resolve,
/// [`AddNoteError::IdentifierExhausted`] when the greatest existing suffix is `9999`, or
/// [`AddNoteError::Store`] when listing or inserting notes fails.
#[cqrsy::command]
pub async fn execute(
    command: AddNote,
    store: &impl ProjectNoteStore,
    pool: &sqlx::SqlitePool,
    clock: &impl Clock,
) -> Result<AddedNote, AddNoteError> {
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
        .map_err(|error| AddNoteError::Store(Box::new(error)))?;
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
    let created = command.date.unwrap_or_else(|| clock.today());
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
        .map_err(|error| AddNoteError::Store(Box::new(error)))?;
    Ok(AddedNote {
        id: created.id,
        title: created.title,
    })
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use pwf_models::{
        AppDate,
        note::{NoteContent, NoteId, NoteTitle, ProjectNote},
    };

    use super::{AddNote, AddNoteError};
    use crate::testing::{FixedClock, InMemoryStore, insert_project};

    #[derive(Debug, thiserror::Error)]
    #[error("sentinel store failure")]
    struct SentinelStoreError;

    fn note(number: u32, title: &str) -> ProjectNote {
        ProjectNote {
            id: NoteId::try_new(format!("PWF-NOTE-{number:04}")).unwrap(),
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
        insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
        for project_selector in ["pwf", "PWF"] {
            let store = InMemoryStore::default().with_project_notes(
                "pwf",
                vec![note(2, "two"), note(9, "nine"), note(4, "four")],
            );

            let added = super::execute(command(project_selector), &store, &pool, &FixedClock)
                .await
                .unwrap();

            assert_eq!(added.id.as_ref(), "PWF-NOTE-0010");
            assert_eq!(added.title.as_ref(), "remember milk");
            assert_eq!(
                store.project_notes("pwf").last().unwrap().id.as_ref(),
                "PWF-NOTE-0010"
            );
        }
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn explicit_date_overrides_the_clock(pool: sqlx::SqlitePool) {
        insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
        let store = InMemoryStore::default();

        super::execute(command("pwf"), &store, &pool, &FixedClock)
            .await
            .unwrap();

        assert_eq!(
            store.project_note_creations("pwf"),
            vec!["2026-07-15".parse::<AppDate>().unwrap()]
        );
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn absent_date_uses_the_clock_once(pool: sqlx::SqlitePool) {
        insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
        let store = InMemoryStore::default();
        let mut command = command("pwf");
        command.date = None;

        super::execute(command, &store, &pool, &FixedClock)
            .await
            .unwrap();

        assert_eq!(
            store.project_note_creations("pwf"),
            vec!["2026-07-26".parse::<AppDate>().unwrap()]
        );
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn exhausted_four_digit_suffix_is_reported_without_inserting(pool: sqlx::SqlitePool) {
        insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
        let store = InMemoryStore::default().with_project_notes("pwf", vec![note(9_999, "last")]);

        let error = super::execute(command("pwf"), &store, &pool, &FixedClock)
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            AddNoteError::IdentifierExhausted { ref project } if project.as_ref() == "pwf"
        ));
        assert_eq!(store.project_notes("pwf"), vec![note(9_999, "last")]);
    }

    #[test]
    fn store_error_preserves_display_and_source() {
        let error = AddNoteError::Store(Box::new(SentinelStoreError));

        assert_eq!(error.to_string(), "sentinel store failure");
        let source = error.source().expect("store error retains its source");
        assert!(source.downcast_ref::<SentinelStoreError>().is_some());
        assert_eq!(source.to_string(), "sentinel store failure");
    }
}
