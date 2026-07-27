use std::path::Path;

use pwf_domain::pending_work::{
    EffortTier, ProjectName, Tags, Timestamp, WorkItemId, WorkItemStatus,
};

use crate::{
    AppRecordStore, Materialization, NoteMarkdownSource, PendingWorkItem,
    pending_work::{project_registry::ProjectRegistry, resolve::resolve_record},
};

mod data;

/// Selects the representation returned by [`execute`].
///
/// # Examples
///
/// ```
/// use pwf_application::pending_work::ShowOutput;
///
/// assert_eq!(ShowOutput::Path, ShowOutput::Path);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShowOutput {
    /// Returns the persisted Markdown source byte-for-byte.
    Markdown,
    /// Returns the record's display path without reading its note.
    Path,
    /// Returns typed task data for machine-readable rendering.
    Json,
}

/// Contains one pending-work item's semantic data.
///
/// # Examples
///
/// ```
/// use pwf_application::pending_work::show_pending_work_item::PendingWorkItemData;
///
/// # fn inspect(task: &PendingWorkItemData) {
/// assert!(!task.id.is_empty());
/// assert!(!task.project.as_ref().is_empty());
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingWorkItemData {
    /// Canonical item id, or `<project>:<ordinal>` for an inline item.
    pub id: String,
    /// Managed project containing the item.
    pub project: ProjectName,
    /// Persisted title.
    pub title: String,
    /// Persisted lifecycle status.
    pub status: WorkItemStatus,
    /// Persisted creation date.
    pub created: Option<Timestamp>,
    /// Persisted completion date.
    pub completed: Option<Timestamp>,
    /// Persisted commit provenance.
    pub commits: Option<String>,
    /// Canonical task labels.
    pub tags: Option<Tags>,
    /// Validated effort tier.
    pub effort: Option<EffortTier>,
    /// Canonical prerequisite identifiers when the relationship is present.
    pub prerequisites: Option<Vec<WorkItemId>>,
    /// Index section containing the item.
    pub section: Option<String>,
    /// Trimmed authored prompt.
    pub prompt: String,
}

/// Returns the selected pending-work representation.
///
/// # Examples
///
/// ```
/// use pwf_application::pending_work::show_pending_work_item::ShowPendingWorkItemOutput;
///
/// # fn consume(output: ShowPendingWorkItemOutput) {
/// if let ShowPendingWorkItemOutput::Json(task) = output {
///     assert!(!task.id.is_empty());
/// }
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShowPendingWorkItemOutput {
    /// Contains persisted Markdown.
    Markdown(String),
    /// Contains the item display path.
    Path(String),
    /// Contains semantic task data.
    Json(Box<PendingWorkItemData>),
}

/// Requests one pending-work item in a selected output representation.
#[derive(Debug, Clone)]
pub struct ShowPendingWorkItem {
    /// Identifier spelling preserved for unmatched-id diagnostics.
    pub id: String,
    /// Representation returned by [`execute`].
    pub output: ShowOutput,
}

/// Reports a pending-work resolution or Markdown read failure.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ShowPendingWorkError {
    /// Preserves the unmatched requested id without normalizing it again.
    #[error("Open pending-work item not found: {id}")]
    ItemNotFound {
        /// Identifier supplied by the caller.
        id: String,
    },
    /// Retains the record store's read or parse failure.
    #[error("{0}")]
    ReadStore(#[source] Box<dyn std::error::Error + Send + Sync>),
    /// Retains the Markdown source's byte-read failure.
    #[error("{0}")]
    ReadMarkdown(#[source] Box<dyn std::error::Error + Send + Sync>),
    /// Reports invalid persisted data required by the typed representation.
    #[error("Invalid pending-work item {field}: {reason}")]
    InvalidItemData {
        /// Semantic field that could not be projected.
        field: &'static str,
        /// Validation failure.
        reason: String,
    },
}

/// Returns a pending-work item's selected representation.
///
/// # Errors
///
/// Returns [`ShowPendingWorkError::ItemNotFound`] when the identifier cannot be resolved,
/// [`ShowPendingWorkError::ReadStore`] when record resolution fails, or
/// [`ShowPendingWorkError::ReadMarkdown`] when a missing-note link cannot be read, or
/// [`ShowPendingWorkError::InvalidItemData`] when persisted task data cannot be projected.
#[cqrsy::query]
pub fn execute<S, N>(
    query: &ShowPendingWorkItem,
    store: &S,
    projects: &ProjectRegistry,
    markdown_source: &N,
) -> Result<ShowPendingWorkItemOutput, ShowPendingWorkError>
where
    S: AppRecordStore<PendingWorkItem>,
    N: NoteMarkdownSource,
{
    let (project, record) = resolve_record(store, projects, &query.id)?;
    match query.output {
        ShowOutput::Path => Ok(ShowPendingWorkItemOutput::Path(record.locator)),
        ShowOutput::Markdown
            if matches!(record.materialization, Materialization::MissingNote { .. }) =>
        {
            markdown_source
                .read_note_markdown(Path::new(&record.locator))
                .map(ShowPendingWorkItemOutput::Markdown)
                .map_err(|error| ShowPendingWorkError::ReadMarkdown(Box::new(error)))
        }
        ShowOutput::Markdown => Ok(ShowPendingWorkItemOutput::Markdown(record.source)),
        ShowOutput::Json => data::from_record(project, record)
            .map(Box::new)
            .map(ShowPendingWorkItemOutput::Json),
    }
}

#[cfg(test)]
mod tests {
    use std::{convert::Infallible, path::Path};

    use super::{ShowOutput, ShowPendingWorkItem, ShowPendingWorkItemOutput};
    use crate::{
        NoteMarkdownSource,
        pending_work::resolve::testing::{PWF_0001_SOURCE, staged, staged_ghost},
    };

    #[derive(Debug, Clone)]
    struct UnusedNoteMarkdownSource;

    impl NoteMarkdownSource for UnusedNoteMarkdownSource {
        type Error = Infallible;

        fn read_note_markdown(&self, _path: &Path) -> Result<String, Self::Error> {
            panic!("selected show output must not read note Markdown")
        }
    }

    #[derive(Debug, Clone, thiserror::Error)]
    #[error("Cannot read item file: staged read failure")]
    struct StagedNoteReadError;

    #[derive(Debug, Clone)]
    struct FailingNoteMarkdownSource;

    impl NoteMarkdownSource for FailingNoteMarkdownSource {
        type Error = StagedNoteReadError;

        fn read_note_markdown(&self, _path: &Path) -> Result<String, Self::Error> {
            Err(StagedNoteReadError)
        }
    }

    #[test]
    fn show_streams_source_verbatim() {
        let (store, registry) = staged();

        let shown = super::execute(
            &ShowPendingWorkItem {
                id: "PWF-0001".to_string(),
                output: ShowOutput::Markdown,
            },
            &store,
            &registry,
            &UnusedNoteMarkdownSource,
        )
        .unwrap();

        assert_eq!(
            shown,
            ShowPendingWorkItemOutput::Markdown(PWF_0001_SOURCE.to_string())
        );
    }

    #[test]
    fn show_path_returns_missing_note_locator_without_reading_markdown() {
        let (store, registry) = staged_ghost();

        let shown = super::execute(
            &ShowPendingWorkItem {
                id: "PWF-0002".to_string(),
                output: ShowOutput::Path,
            },
            &store,
            &registry,
            &UnusedNoteMarkdownSource,
        )
        .unwrap();

        assert_eq!(
            shown,
            ShowPendingWorkItemOutput::Path("/notes/pwf/PWF-0002.md".to_string())
        );
    }

    #[test]
    fn show_markdown_preserves_missing_note_source_error() {
        let (store, registry) = staged_ghost();

        let error = super::execute(
            &ShowPendingWorkItem {
                id: "PWF-0002".to_string(),
                output: ShowOutput::Markdown,
            },
            &store,
            &registry,
            &FailingNoteMarkdownSource,
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "Cannot read item file: staged read failure"
        );
    }
}
