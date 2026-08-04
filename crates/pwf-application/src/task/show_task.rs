use pwf_models::task::{EffortTier, ProjectName, Tags, TaskId, TaskStatus, Timestamp};

use crate::{
    ports::{
        project_note::ProjectNoteStore,
        task_record::{Materialization, TaskRecord, TaskStore},
    },
    project::{
        get_active_project::{self, GetActiveProject},
        get_project::GetProjectError,
    },
    task::{prerequisites, tags},
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
    /// Task ID.
    pub id: TaskId,
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
    /// Task labels.
    pub tags: Option<Tags>,
    /// Validated effort tier.
    pub effort: Option<EffortTier>,
    /// Prerequisite IDs when the relationship is present.
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
    /// Task ID.
    pub id: TaskId,
    /// Representation returned by [`execute`].
    pub output: ShowOutput,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ShowTaskError {
    #[error("Task not found: {id}")]
    TaskNotFound { id: TaskId },
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
    let project = match get_active_project::execute(
        GetActiveProject {
            id: query.id.project_id(),
        },
        pool,
    )
    .await
    {
        Ok(project) => project,
        Err(GetProjectError::ProjectNotFound { .. }) => {
            return Err(ShowTaskError::TaskNotFound {
                id: query.id.clone(),
            });
        }
        Err(error) => return Err(ShowTaskError::QueryProject(Box::new(error))),
    };
    let record = store
        .get(&project, &query.id)
        .map_err(|error| ShowTaskError::ReadStore(Box::new(error)))?
        .ok_or_else(|| ShowTaskError::TaskNotFound {
            id: query.id.clone(),
        })?;
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
        .map(tags::parse_frontmatter)
        .transpose()
        .map_err(|error| invalid_task_data("tags", error))?;
    let effort = record.effort.as_deref().map(parse_effort).transpose()?;
    let prerequisites = record
        .prereq
        .as_deref()
        .map(prerequisites::parse_frontmatter)
        .transpose()
        .map_err(|error| invalid_task_data("prerequisites", error))?;
    Ok(TaskData {
        id: record.id,
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
    use super::{ShowOutput, ShowTask, ShowTaskOk, TaskId};
    use crate::testing::{
        PWF_0001_SOURCE, ProjectNoteFailure, insert_project, staged_missing_task, staged_task,
    };

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn show_streams_source_verbatim(pool: sqlx::SqlitePool) {
        insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
        let (store, _) = staged_task();
        let query = ShowTask {
            id: TaskId::try_new("PWF-0001").unwrap(),
            output: ShowOutput::Markdown,
        };

        let shown = super::execute(&query, &store, &pool).await.unwrap();

        assert_eq!(shown, ShowTaskOk::Markdown(PWF_0001_SOURCE.to_string()));
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn show_path_returns_missing_note_locator_without_reading_markdown(
        pool: sqlx::SqlitePool,
    ) {
        insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
        let (store, _) = staged_missing_task();
        let query = ShowTask {
            id: TaskId::try_new("PWF-0002").unwrap(),
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
        insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
        let (store, _) = staged_missing_task();
        let store = store.with_failure(ProjectNoteFailure::Read);
        let query = ShowTask {
            id: TaskId::try_new("PWF-0002").unwrap(),
            output: ShowOutput::Markdown,
        };

        let error = super::execute(&query, &store, &pool).await.unwrap_err();

        assert_eq!(
            error.to_string(),
            "injected in-memory store failure: project-note-read"
        );
    }
}
