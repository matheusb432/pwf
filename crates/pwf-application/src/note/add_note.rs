//! Adds one note to a managed project.

use pwf_models::{note::NoteId, task::Timestamp};

use crate::{
    ports::{
        clock::Clock,
        project_note::{NewProjectNote, ProjectNoteStore},
    },
    project::{
        ProjectStatusFilter,
        resolve_project::{self, ResolveProject, ResolveProjectError},
    },
};

/// Requests creation of one project note.
#[derive(Debug, Clone)]
pub struct AddNote {
    /// Selects the managed project by name or id code.
    pub project_identifier: String,
    /// Names the note.
    pub title: String,
    /// Supplies the note's Markdown body.
    pub content: String,
    /// Explains the consequence when it adds useful context.
    pub why: Option<String>,
    /// Classifies the subject when known.
    pub domain: Option<String>,
    /// Supplies discovery labels.
    pub tags: Vec<String>,
    /// Records supporting evidence.
    pub sources: Vec<String>,
    /// Records the supplied verification marker.
    pub verified: Option<String>,
    /// Overrides the clock date when present.
    pub date: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddNoteOk {
    pub id: NoteId,
    pub title: String,
}

#[derive(Debug, thiserror::Error)]
pub enum AddNoteError {
    #[error("Unknown project '{identifier}'; expected a managed project name or id code.")]
    UnknownProject { identifier: String },
    #[error("Note title is empty; provide a non-empty title.")]
    EmptyTitle,
    #[error("Note content is empty; provide non-empty content.")]
    EmptyContent,
    #[error("Project '{project}' has no available four-digit note identifiers.")]
    IdentifierExhausted { project: String },
    #[error("{0}")]
    Project(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("{0}")]
    Store(#[source] Box<dyn std::error::Error + Send + Sync>),
}

/// Validates, allocates, and persists one project note.
///
/// # Errors
///
/// Returns [`AddNoteError::UnknownProject`] when the project does not resolve,
/// [`AddNoteError::EmptyTitle`] when the normalized title is empty,
/// [`AddNoteError::EmptyContent`] when the trimmed content is empty,
/// [`AddNoteError::IdentifierExhausted`] when the greatest existing suffix is `9999`, or
/// [`AddNoteError::Store`] when listing or inserting notes fails.
#[cqrsy::command]
pub async fn execute(
    command: AddNote,
    store: &impl ProjectNoteStore,
    pool: &sqlx::SqlitePool,
    clock: &impl Clock,
) -> Result<AddNoteOk, AddNoteError> {
    let identifier = command.project_identifier.clone();
    let project = resolve_project::execute(
        ResolveProject {
            identifier: identifier.clone(),
            status: ProjectStatusFilter::ACTIVE,
        },
        pool,
    )
    .await
    .map_err(|error| project_error(identifier, error))?;
    let title = normalize_inline(&command.title);
    if title.is_empty() {
        return Err(AddNoteError::EmptyTitle);
    }
    let content = command.content.trim().to_string();
    if content.is_empty() {
        return Err(AddNoteError::EmptyContent);
    }
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
            project: project.title.to_string(),
        })?;
    let id = NoteId::try_new(format!("{}-NOTE-{next_number:04}", project.id)).map_err(|_| {
        AddNoteError::IdentifierExhausted {
            project: project.title.to_string(),
        }
    })?;
    let created = command.date.map_or_else(|| clock.today(), Timestamp::new);
    let created = store
        .insert_note(
            &project,
            NewProjectNote {
                id,
                title: title.clone(),
                content,
                why: normalize_optional_block(command.why),
                domain: normalize_optional_inline(command.domain),
                tags: normalize_inline_values(command.tags),
                sources: normalize_inline_values(command.sources),
                verified: normalize_optional_inline(command.verified),
                created,
            },
        )
        .map_err(|error| AddNoteError::Store(Box::new(error)))?;
    Ok(AddNoteOk {
        id: created.id,
        title: created.title,
    })
}

fn project_error(identifier: String, error: ResolveProjectError) -> AddNoteError {
    match error {
        ResolveProjectError::Unknown { .. } => AddNoteError::UnknownProject { identifier },
        error @ ResolveProjectError::Unexpected { .. } => AddNoteError::Project(Box::new(error)),
    }
}

fn normalize_inline(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalize_optional_inline(value: Option<String>) -> Option<String> {
    value
        .map(|value| normalize_inline(&value))
        .filter(|value| !value.is_empty())
}

fn normalize_optional_block(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn normalize_inline_values(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .map(|value| normalize_inline(&value))
        .filter(|value| !value.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use pwf_models::{
        note::{NoteId, ProjectNote},
        task::Timestamp,
    };

    use super::{AddNote, AddNoteError};
    use crate::{
        ports::clock::Clock,
        testing::{InMemoryStore, ProjectNoteFailure, insert_project},
    };

    #[derive(Debug, thiserror::Error)]
    #[error("sentinel store failure")]
    struct SentinelStoreError;

    #[derive(Clone)]
    struct FixedClock;

    impl Clock for FixedClock {
        fn today(&self) -> Timestamp {
            Timestamp::new("2026-07-26")
        }
    }

    fn note(number: u32, title: &str) -> ProjectNote {
        ProjectNote {
            id: NoteId::try_new(format!("PWF-NOTE-{number:04}")).unwrap(),
            title: title.to_string(),
        }
    }

    fn command(project_identifier: &str) -> AddNote {
        AddNote {
            project_identifier: project_identifier.to_string(),
            title: " remember milk ".to_string(),
            content: " buy milk before the store closes ".to_string(),
            why: None,
            domain: None,
            tags: Vec::new(),
            sources: Vec::new(),
            verified: None,
            date: Some("2026-07-15".to_string()),
        }
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn blank_title_wins_over_store_listing_failure(pool: sqlx::SqlitePool) {
        insert_project(&pool, "PWF", "pwf", "/repo/pwf", "/tasks/pwf", false).await;
        let store = InMemoryStore::default().with_failure(ProjectNoteFailure::List);
        let mut command = command("pwf");
        command.title = " \t ".to_string();

        let error = super::execute(command, &store, &pool, &FixedClock)
            .await
            .unwrap_err();

        assert!(matches!(error, AddNoteError::EmptyTitle));
        assert!(store.project_notes("pwf").is_empty());
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn blank_content_wins_over_store_listing_failure(pool: sqlx::SqlitePool) {
        insert_project(&pool, "PWF", "pwf", "/repo/pwf", "/tasks/pwf", false).await;
        let store = InMemoryStore::default().with_failure(ProjectNoteFailure::List);
        let mut command = command("pwf");
        command.content = " \t ".to_string();

        let error = super::execute(command, &store, &pool, &FixedClock)
            .await
            .unwrap_err();

        assert!(matches!(error, AddNoteError::EmptyContent));
        assert!(store.project_notes("pwf").is_empty());
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn maximum_suffix_allocates_the_next_identifier(pool: sqlx::SqlitePool) {
        insert_project(&pool, "PWF", "pwf", "/repo/pwf", "/tasks/pwf", false).await;
        for project_identifier in ["pwf", "PWF"] {
            let store = InMemoryStore::default().with_project_notes(
                "pwf",
                vec![note(2, "two"), note(9, "nine"), note(4, "four")],
            );

            let added = super::execute(command(project_identifier), &store, &pool, &FixedClock)
                .await
                .unwrap();

            assert_eq!(added.id.as_ref(), "PWF-NOTE-0010");
            assert_eq!(added.title, "remember milk");
            assert_eq!(
                store.project_notes("pwf").last().unwrap().id.as_ref(),
                "PWF-NOTE-0010"
            );
        }
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn explicit_date_overrides_the_clock(pool: sqlx::SqlitePool) {
        insert_project(&pool, "PWF", "pwf", "/repo/pwf", "/tasks/pwf", false).await;
        let store = InMemoryStore::default();

        super::execute(command("pwf"), &store, &pool, &FixedClock)
            .await
            .unwrap();

        assert_eq!(
            store.project_note_creations("pwf"),
            vec![Timestamp::new("2026-07-15")]
        );
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn absent_date_uses_the_clock_once(pool: sqlx::SqlitePool) {
        insert_project(&pool, "PWF", "pwf", "/repo/pwf", "/tasks/pwf", false).await;
        let store = InMemoryStore::default();
        let mut command = command("pwf");
        command.date = None;

        super::execute(command, &store, &pool, &FixedClock)
            .await
            .unwrap();

        assert_eq!(
            store.project_note_creations("pwf"),
            vec![Timestamp::new("2026-07-26")]
        );
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn exhausted_four_digit_suffix_is_reported_without_inserting(pool: sqlx::SqlitePool) {
        insert_project(&pool, "PWF", "pwf", "/repo/pwf", "/tasks/pwf", false).await;
        let store = InMemoryStore::default().with_project_notes("pwf", vec![note(9_999, "last")]);

        let error = super::execute(command("pwf"), &store, &pool, &FixedClock)
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            AddNoteError::IdentifierExhausted { ref project } if project == "pwf"
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
