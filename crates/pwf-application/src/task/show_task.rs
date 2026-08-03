use pwf_models::task::{EffortTier, ProjectName, Tags, TaskId, TaskStatus, Timestamp};

use super::identifier;
use crate::{
    ports::{
        project_note::ProjectNoteStore,
        task_record::{Materialization, RecordId, TaskRecord, TaskStore},
    },
    project::{
        ProjectStatusFilter,
        get_active_project::{self, GetActiveProject},
        get_project::GetProjectError,
        list_projects::{self, ListProjects},
    },
    task::logic::{prerequisite, resolve::resolve_record_in_projects, tag_policy},
};

/// Selects the representation returned by [`execute`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShowOutput {
    /// Returns the persisted Markdown source byte-for-byte.
    Markdown,
    /// Returns the record's display path without reading its note.
    Path,
    /// Returns typed task data for machine-readable rendering.
    Json,
}

/// Contains one task's semantic data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskData {
    /// Canonical task id, or `<project>:<ordinal>` for an inline task.
    pub id: String,
    /// Managed project containing the task.
    pub project: ProjectName,
    /// Persisted title.
    pub title: String,
    /// Persisted lifecycle status.
    pub status: TaskStatus,
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
    pub prerequisites: Option<Vec<TaskId>>,
    /// Index section containing the task.
    pub section: Option<String>,
    /// Trimmed authored prompt.
    pub prompt: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShowTaskOk {
    Markdown(String),
    Path(String),
    Json(Box<TaskData>),
}

/// Requests one task in a selected output representation.
#[derive(Debug, Clone)]
pub struct ShowTask {
    /// Identifier spelling preserved for unmatched-id diagnostics.
    pub id: String,
    /// Representation returned by [`execute`].
    pub output: ShowOutput,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ShowTaskError {
    #[error("Task not found: {id}")]
    TaskNotFound { id: String },
    #[error("{0}")]
    ReadStore(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("{0}")]
    ReadMarkdown(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("{0}")]
    QueryProject(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("Invalid task {field}: {reason}")]
    InvalidTaskData { field: &'static str, reason: String },
}

/// Returns a task's selected representation.
///
/// # Errors
///
/// Returns [`ShowTaskError::TaskNotFound`] when the identifier cannot be resolved,
/// [`ShowTaskError::ReadStore`] when record resolution fails, or
/// [`ShowTaskError::ReadMarkdown`] when a missing-note link cannot be read, or
/// [`ShowTaskError::InvalidTaskData`] when persisted task data cannot be projected.
#[cqrsy::query]
pub async fn execute(
    query: &ShowTask,
    store: &(impl TaskStore + ProjectNoteStore),
    pool: &sqlx::SqlitePool,
) -> Result<ShowTaskOk, ShowTaskError> {
    let projects = match identifier::parse(&query.id) {
        Some(task_id) => match get_active_project::execute(
            GetActiveProject {
                id: task_id.project_id(),
            },
            pool,
        )
        .await
        {
            Ok(project) => vec![project],
            Err(GetProjectError::ProjectNotFound { .. }) => {
                return Err(ShowTaskError::TaskNotFound {
                    id: query.id.clone(),
                });
            }
            Err(error) => return Err(ShowTaskError::QueryProject(Box::new(error))),
        },
        None => list_projects::execute(
            ListProjects {
                status: ProjectStatusFilter::ACTIVE,
            },
            pool,
        )
        .await
        .map_err(|error| ShowTaskError::QueryProject(Box::new(error)))?,
    };
    let (project, record) = resolve_record_in_projects(store, &projects, &query.id)?;
    match query.output {
        ShowOutput::Path => Ok(ShowTaskOk::Path(record.locator)),
        ShowOutput::Markdown
            if matches!(record.materialization, Materialization::MissingNote { .. }) =>
        {
            store
                .read_note_markdown(&record.locator)
                .map(ShowTaskOk::Markdown)
                .map_err(|error| ShowTaskError::ReadMarkdown(Box::new(error)))
        }
        ShowOutput::Markdown => Ok(ShowTaskOk::Markdown(record.source)),
        ShowOutput::Json => task_data(project.title, record)
            .map(Box::new)
            .map(ShowTaskOk::Json),
    }
}

fn task_data(project: ProjectName, record: TaskRecord) -> Result<TaskData, ShowTaskError> {
    let tags = record
        .tags
        .as_deref()
        .map(tag_policy::parse_frontmatter)
        .transpose()
        .map_err(|error| invalid_task_data("tags", error))?;
    let effort = record.effort.as_deref().map(parse_effort).transpose()?;
    let prerequisites = record
        .prereq
        .as_deref()
        .map(prerequisite::parse_frontmatter)
        .transpose()
        .map_err(|error| invalid_task_data("prerequisites", error))?;
    let id = match record.id {
        RecordId::Task(id) => id.to_string(),
        RecordId::Inline(ordinal) => format!("{project}:{ordinal}"),
    };

    Ok(TaskData {
        id,
        project,
        title: record.title,
        status: record.status,
        created: record.created,
        completed: record.completed,
        commits: record
            .commits
            .map(|value| unquote_scalar(&value).to_string()),
        tags,
        effort,
        prerequisites,
        section: record.section,
        prompt: record.body.trim().to_string(),
    })
}

fn unquote_scalar(raw: &str) -> &str {
    raw.strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            raw.strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(raw)
}

fn parse_effort(raw: &str) -> Result<EffortTier, ShowTaskError> {
    raw.trim()
        .parse()
        .map_err(|error| invalid_task_data("effort", error))
}

fn invalid_task_data(field: &'static str, error: impl std::fmt::Display) -> ShowTaskError {
    ShowTaskError::InvalidTaskData {
        field,
        reason: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{ShowOutput, ShowTask, ShowTaskOk};
    use crate::{
        task::logic::resolve::testing::{PWF_0001_SOURCE, staged, staged_ghost},
        testing::{ProjectNoteFailure, insert_project},
    };

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn show_streams_source_verbatim(pool: sqlx::SqlitePool) {
        insert_project(&pool, "PWF", "pwf", "/repo/pwf", "/tasks/pwf", false).await;
        let (store, _) = staged();
        let query = ShowTask {
            id: "PWF-0001".to_string(),
            output: ShowOutput::Markdown,
        };

        let shown = super::execute(&query, &store, &pool).await.unwrap();

        assert_eq!(shown, ShowTaskOk::Markdown(PWF_0001_SOURCE.to_string()));
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn show_path_returns_missing_note_locator_without_reading_markdown(
        pool: sqlx::SqlitePool,
    ) {
        insert_project(&pool, "PWF", "pwf", "/repo/pwf", "/tasks/pwf", false).await;
        let (store, _) = staged_ghost();
        let query = ShowTask {
            id: "PWF-0002".to_string(),
            output: ShowOutput::Path,
        };

        let shown = super::execute(&query, &store, &pool).await.unwrap();

        assert_eq!(
            shown,
            ShowTaskOk::Path("/notes/pwf/PWF-0002.md".to_string())
        );
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn show_markdown_preserves_missing_note_source_error(pool: sqlx::SqlitePool) {
        insert_project(&pool, "PWF", "pwf", "/repo/pwf", "/tasks/pwf", false).await;
        let (store, _) = staged_ghost();
        let store = store.with_failure(ProjectNoteFailure::Read);
        let query = ShowTask {
            id: "PWF-0002".to_string(),
            output: ShowOutput::Markdown,
        };

        let error = super::execute(&query, &store, &pool).await.unwrap_err();

        assert_eq!(
            error.to_string(),
            "injected in-memory store failure: project-note-read"
        );
    }
}
