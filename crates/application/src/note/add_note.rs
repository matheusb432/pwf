//! Adds one note to a managed project.

use pwf_models::{note::NoteId, pending_work::Timestamp};

use super::{identifier, identifier::ResolvedProject};
use crate::{AppRecordStore, Clock, NewProjectNote, ProjectNote, pending_work::ProjectRegistry};

/// Requests creation of one project note.
///
/// # Examples
///
/// ```
/// use pwf_application::note::add_note::AddNote;
///
/// let request = AddNote {
///     project_identifier: "pwf".to_string(),
///     message: "remember milk".to_string(),
///     date: Some("2026-07-26".to_string()),
/// };
/// assert_eq!(request.project_identifier, "pwf");
/// ```
#[derive(Debug, Clone)]
pub struct AddNote {
    /// Selects the managed project by name or id code.
    pub project_identifier: String,
    /// Supplies the one-line note message.
    pub message: String,
    /// Overrides the clock date when present.
    pub date: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddNoteOk {
    pub id: NoteId,
    pub message: String,
}

#[derive(Debug, thiserror::Error)]
pub enum AddNoteError {
    #[error("Unknown project '{identifier}'; expected a managed project name or id code.")]
    UnknownProject { identifier: String },
    #[error("Note message is empty; provide a non-empty message.")]
    EmptyMessage,
    #[error("Project '{project}' has no available four-digit note identifiers.")]
    IdentifierExhausted { project: String },
    #[error("{0}")]
    Store(#[source] Box<dyn std::error::Error + Send + Sync>),
}

/// Validates, allocates, and persists one project note.
///
/// # Errors
///
/// Returns [`AddNoteError::UnknownProject`] when the project does not resolve,
/// [`AddNoteError::EmptyMessage`] when the trimmed message is empty,
/// [`AddNoteError::IdentifierExhausted`] when the greatest existing suffix is `9999`, or
/// [`AddNoteError::Store`] when listing or inserting notes fails.
///
/// # Examples
///
/// ```
/// # use pwf_application::{
/// #     AppRecordStore, Clock,
/// #     note::add_note::{self, AddNote, AddNoteError, AddNoteOk},
/// #     pending_work::ProjectRegistry,
/// # };
/// # use pwf_models::note::ProjectNote;
/// # fn add<S, C>(
/// #     request: AddNote,
/// #     store: &S,
/// #     projects: &ProjectRegistry,
/// #     clock: &C,
/// # ) -> Result<AddNoteOk, AddNoteError>
/// # where
/// #     S: AppRecordStore<ProjectNote>,
/// #     C: Clock,
/// # {
/// add_note::execute(request, store, projects, clock)
/// # }
/// ```
#[cqrsy::command]
pub fn execute<S, C>(
    command: AddNote,
    store: &S,
    projects: &ProjectRegistry,
    clock: &C,
) -> Result<AddNoteOk, AddNoteError>
where
    S: AppRecordStore<ProjectNote>,
    C: Clock,
{
    let ResolvedProject { project, prefix } =
        identifier::resolve_project(projects, &command.project_identifier).ok_or_else(|| {
            AddNoteError::UnknownProject {
                identifier: command.project_identifier.clone(),
            }
        })?;
    let message = command.message.trim();
    if message.is_empty() {
        return Err(AddNoteError::EmptyMessage);
    }
    let notes = store
        .list(&project)
        .map_err(|error| AddNoteError::Store(Box::new(error)))?;
    let next_number = notes
        .iter()
        .map(|note| note.id.number())
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .filter(|number| *number <= 9_999)
        .ok_or_else(|| AddNoteError::IdentifierExhausted {
            project: project.to_string(),
        })?;
    let id = NoteId::try_new(format!("{prefix}-NOTE-{next_number:04}")).map_err(|_| {
        AddNoteError::IdentifierExhausted {
            project: project.to_string(),
        }
    })?;
    let created = command.date.map_or_else(|| clock.today(), Timestamp::new);
    let created = store
        .insert(
            &project,
            NewProjectNote {
                id,
                message: message.to_string(),
                created,
            },
        )
        .map_err(|error| AddNoteError::Store(Box::new(error)))?;
    Ok(AddNoteOk {
        id: created.id,
        message: created.message,
    })
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use pwf_models::{
        note::NoteId,
        pending_work::{ProjectName, Timestamp},
    };

    use super::{AddNote, AddNoteError};
    use crate::{
        Clock, ProjectNote,
        pending_work::ProjectRegistry,
        testing::{FailurePoint, InMemoryStore},
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

    fn registry() -> ProjectRegistry {
        ProjectRegistry::new([(
            ProjectName::try_new("pwf").unwrap(),
            Some("/repo/pwf".to_string()),
            Some("PWF".to_string()),
        )])
    }

    fn note(number: u32, message: &str) -> ProjectNote {
        ProjectNote {
            id: NoteId::try_new(format!("PWF-NOTE-{number:04}")).unwrap(),
            message: message.to_string(),
        }
    }

    fn command(project_identifier: &str) -> AddNote {
        AddNote {
            project_identifier: project_identifier.to_string(),
            message: " remember milk ".to_string(),
            date: Some("2026-07-15".to_string()),
        }
    }

    #[test]
    fn blank_message_wins_over_store_listing_failure() {
        let store = InMemoryStore::default().with_failure(FailurePoint::ProjectNoteList);
        let mut command = command("pwf");
        command.message = " \t ".to_string();

        let error = super::execute(command, &store, &registry(), &FixedClock).unwrap_err();

        assert!(matches!(error, AddNoteError::EmptyMessage));
        assert!(store.project_notes("pwf").is_empty());
    }

    #[test]
    fn project_name_and_prefix_resolve_to_maximum_suffix_plus_one() {
        for project_identifier in ["pwf", "PWF"] {
            let store = InMemoryStore::default().with_project_notes(
                "pwf",
                vec![note(2, "two"), note(9, "nine"), note(4, "four")],
            );

            let added = super::execute(
                command(project_identifier),
                &store,
                &registry(),
                &FixedClock,
            )
            .unwrap();

            assert_eq!(added.id.as_ref(), "PWF-NOTE-0010");
            assert_eq!(added.message, "remember milk");
            assert_eq!(
                store.project_notes("pwf").last().unwrap().id.as_ref(),
                "PWF-NOTE-0010"
            );
        }
    }

    #[test]
    fn explicit_date_overrides_the_clock() {
        let store = InMemoryStore::default();

        super::execute(command("pwf"), &store, &registry(), &FixedClock).unwrap();

        assert_eq!(
            store.project_note_creations("pwf"),
            vec![Timestamp::new("2026-07-15")]
        );
    }

    #[test]
    fn absent_date_uses_the_clock_once() {
        let store = InMemoryStore::default();
        let mut command = command("pwf");
        command.date = None;

        super::execute(command, &store, &registry(), &FixedClock).unwrap();

        assert_eq!(
            store.project_note_creations("pwf"),
            vec![Timestamp::new("2026-07-26")]
        );
    }

    #[test]
    fn exhausted_four_digit_suffix_is_reported_without_inserting() {
        let store = InMemoryStore::default().with_project_notes("pwf", vec![note(9_999, "last")]);

        let error = super::execute(command("pwf"), &store, &registry(), &FixedClock).unwrap_err();

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
