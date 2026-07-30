//! Lists one managed project's notes.

use pwf_models::pending_work::ProjectName;

use super::{
    dto::ListedNote,
    identifier::{self, ResolvedProject},
};
use crate::{AppRecordStore, ProjectNote, pending_work::ProjectRegistry};

const DEFAULT_NOTE_COUNT: usize = 10;

/// Requests one project's notes in newest-first order.
///
/// # Examples
///
/// ```
/// use pwf_application::note::list_notes::ListNotes;
///
/// let query = ListNotes {
///     project_identifier: "pwf".to_string(),
///     number: Some(20),
/// };
/// assert_eq!(query.number, Some(20));
/// ```
#[derive(Debug, Clone)]
pub struct ListNotes {
    /// Selects the managed project by name or id code.
    pub project_identifier: String,
    /// Caps returned notes, with `None` selecting ten and `Some(0)` selecting all.
    pub number: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListNotesOk {
    pub project: ProjectName,
    pub notes: Vec<ListedNote>,
    pub hidden: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum ListNotesError {
    #[error("Unknown project '{identifier}'; expected a managed project name or id code.")]
    UnknownProject { identifier: String },
    #[error("{0}")]
    Store(#[source] Box<dyn std::error::Error + Send + Sync>),
}

/// Reads, orders, and caps one project's notes.
///
/// # Errors
///
/// Returns [`ListNotesError::UnknownProject`] when the project does not resolve or
/// [`ListNotesError::Store`] when listing notes fails.
///
/// # Examples
///
/// ```
/// # use pwf_application::{
/// #     AppRecordStore,
/// #     note::list_notes::{self, ListNotes, ListNotesError, ListNotesOk},
/// #     pending_work::ProjectRegistry,
/// # };
/// # use pwf_models::note::ProjectNote;
/// # fn list<S>(
/// #     query: ListNotes,
/// #     store: &S,
/// #     projects: &ProjectRegistry,
/// # ) -> Result<ListNotesOk, ListNotesError>
/// # where
/// #     S: AppRecordStore<ProjectNote>,
/// # {
/// list_notes::execute(query, store, projects)
/// # }
/// ```
#[cqrsy::query]
pub fn execute<S>(
    query: ListNotes,
    store: &S,
    projects: &ProjectRegistry,
) -> Result<ListNotesOk, ListNotesError>
where
    S: AppRecordStore<ProjectNote>,
{
    let ListNotes {
        project_identifier,
        number,
    } = query;
    let ResolvedProject { project, prefix: _ } =
        identifier::resolve_project(projects, &project_identifier).ok_or_else(|| {
            ListNotesError::UnknownProject {
                identifier: project_identifier,
            }
        })?;
    let mut notes = store
        .list(&project)
        .map_err(|error| ListNotesError::Store(Box::new(error)))?;
    notes.sort_by_key(|note| std::cmp::Reverse(note.id.number()));
    let count = number.unwrap_or(DEFAULT_NOTE_COUNT);
    let shown = if count == 0 {
        notes.len()
    } else {
        count.min(notes.len())
    };
    let hidden = notes.len() - shown;
    let notes = notes
        .into_iter()
        .take(shown)
        .map(|note| ListedNote {
            id: note.id,
            topic: note.topic,
        })
        .collect();
    Ok(ListNotesOk {
        project,
        notes,
        hidden,
    })
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use pwf_models::{note::NoteId, pending_work::ProjectName};

    use super::{ListNotes, ListNotesError};
    use crate::{ProjectNote, pending_work::ProjectRegistry, testing::InMemoryStore};

    #[derive(Debug, thiserror::Error)]
    #[error("sentinel store failure")]
    struct SentinelStoreError;

    fn registry() -> ProjectRegistry {
        ProjectRegistry::new([(
            ProjectName::try_new("pwf").unwrap(),
            Some("/repo/pwf".to_string()),
            Some("PWF".to_string()),
        )])
    }

    fn note(number: u32) -> ProjectNote {
        ProjectNote {
            id: NoteId::try_new(format!("PWF-NOTE-{number:04}")).unwrap(),
            topic: format!("note {number}"),
        }
    }

    fn identifiers(result: &super::ListNotesOk) -> Vec<&str> {
        result.notes.iter().map(|note| note.id.as_ref()).collect()
    }

    #[test]
    fn list_orders_newest_first_and_defaults_to_ten() {
        let store = InMemoryStore::default().with_project_notes(
            "pwf",
            [1, 12, 5, 3, 11, 8, 2, 10, 7, 4, 9, 6]
                .into_iter()
                .map(note)
                .collect(),
        );

        let result = super::execute(
            ListNotes {
                project_identifier: "PWF".to_string(),
                number: None,
            },
            &store,
            &registry(),
        )
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

    #[test]
    fn zero_is_unlimited_and_explicit_cap_reports_hidden_count() {
        let store = InMemoryStore::default().with_project_notes("pwf", (1..=4).map(note).collect());

        let unlimited = super::execute(
            ListNotes {
                project_identifier: "pwf".to_string(),
                number: Some(0),
            },
            &store,
            &registry(),
        )
        .unwrap();
        let capped = super::execute(
            ListNotes {
                project_identifier: "pwf".to_string(),
                number: Some(2),
            },
            &store,
            &registry(),
        )
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
