use pwf_models::{
    project::ProjectName,
    task::{CommitRanges, EffortTier, TaskId, TaskPrompt, TaskTitle},
};
use pwf_wire::task::{TaskData, TaskNotePath, TaskRead, TaskReadFormat};

use crate::{
    ports::{
        project_note::ProjectNoteStore,
        task_record::{Materialization, TaskRecord, TaskStore},
    },
    project::{
        get_active_project::{self, GetActiveProject},
        get_project::GetProjectError,
    },
    task::{blocked_by, tags},
};

/// Requests one task in a selected output representation.
#[derive(Debug, Clone)]
pub struct GetTask {
    /// Task ID.
    pub id: TaskId,
    /// Representation returned by [`execute`].
    pub output: TaskReadFormat,
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum GetTaskError {
    #[error("Task not found: {id}")]
    TaskNotFound { id: TaskId },
    #[error("{0}")]
    ReadStore(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("{0}")]
    ReadMarkdown(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("{0}")]
    QueryProject(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("Invalid task {field}: {source}")]
    InvalidTaskData {
        field: &'static str,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

/// Returns a task's selected representation.
///
/// # Errors
///
/// Returns [`GetTaskError::TaskNotFound`] when the identifier cannot be resolved,
/// [`GetTaskError::ReadStore`] when record resolution fails, or
/// [`GetTaskError::ReadMarkdown`] when a missing-note link cannot be read, or
/// [`GetTaskError::InvalidTaskData`] when persisted task data cannot be projected.
#[cqrsy::query]
pub async fn execute(
    query: &GetTask,
    store: &(impl TaskStore + ProjectNoteStore),
    pool: &sqlx::SqlitePool,
) -> Result<TaskRead, GetTaskError> {
    let project = match get_active_project::execute(
        GetActiveProject {
            id: query.id.project_id().clone(),
        },
        pool,
    )
    .await
    {
        Ok(project) => project,
        Err(GetProjectError::ProjectNotFound { .. }) => {
            return Err(GetTaskError::TaskNotFound {
                id: query.id.clone(),
            });
        }
        Err(error) => return Err(GetTaskError::QueryProject(Box::new(error))),
    };
    let record = store
        .get(&project, &query.id)
        .map_err(|error| GetTaskError::ReadStore(Box::new(error)))?
        .ok_or_else(|| GetTaskError::TaskNotFound {
            id: query.id.clone(),
        })?;
    match query.output {
        TaskReadFormat::Path => Ok(TaskRead::Path(TaskNotePath::new(record.locator.into()))),
        TaskReadFormat::Markdown
            if matches!(record.materialization, Materialization::MissingNote { .. }) =>
        {
            store
                .read_note_markdown(&record.locator)
                .map(TaskRead::Markdown)
                .map_err(|error| GetTaskError::ReadMarkdown(Box::new(error)))
        }
        TaskReadFormat::Markdown => Ok(TaskRead::Markdown(record.source)),
        TaskReadFormat::Data => task_data(project.title, record)
            .map(Box::new)
            .map(TaskRead::Data),
    }
}

fn task_data(project: ProjectName, record: TaskRecord) -> Result<TaskData, GetTaskError> {
    let title =
        TaskTitle::try_new(record.title).map_err(|error| invalid_task_data("title", error))?;
    let tags = record
        .tags
        .as_ref()
        .map(tags::parse_frontmatter)
        .transpose()
        .map_err(|error| invalid_task_data("tags", error))?;
    let effort = record.effort.as_deref().map(parse_effort).transpose()?;
    let blocked_by = record
        .blocked_by
        .as_deref()
        .map(blocked_by::parse_frontmatter)
        .transpose()
        .map_err(|error| invalid_task_data("blocked_by", error))?;
    let commits = record
        .commits
        .map(|value| CommitRanges::try_new(unquote_scalar(&value).to_string()))
        .transpose()
        .map_err(|error| invalid_task_data("commits", error))?;
    Ok(TaskData {
        id: record.id,
        project,
        title,
        status: record.status,
        created: record.created,
        completed: record.completed,
        commits,
        tags,
        effort,
        blocked_by,
        section: record.section,
        prompt: TaskPrompt::new(record.body.trim()),
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

fn parse_effort(raw: &str) -> Result<EffortTier, GetTaskError> {
    raw.trim()
        .parse()
        .map_err(|error| invalid_task_data("effort", error))
}

fn invalid_task_data(
    field: &'static str,
    source: impl std::error::Error + Send + Sync + 'static,
) -> GetTaskError {
    GetTaskError::InvalidTaskData {
        field,
        source: Box::new(source),
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use pwf_models::task::TaskStatus;
    use pwf_wire::task::{RawTaskTags, TaskNotePath, TaskRead, TaskReadFormat};

    use super::{GetTask, GetTaskError, TaskId};
    use crate::{
        ports::task_record::TaskRecord,
        testing::{
            InMemoryStore, PWF_0001_SOURCE, ProjectNoteFailure, app_date, insert_project,
            staged_missing_task, staged_task, task_record,
        },
    };

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn get_streams_source_verbatim(pool: sqlx::SqlitePool) {
        insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
        let (store, _) = staged_task();
        let query = GetTask {
            id: TaskId::try_new("PWF-0001").unwrap(),
            output: TaskReadFormat::Markdown,
        };

        let gotten = super::execute(&query, &store, &pool).await.unwrap();

        assert_eq!(gotten, TaskRead::Markdown(PWF_0001_SOURCE.to_string()));
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn get_path_returns_missing_note_locator_without_reading_markdown(
        pool: sqlx::SqlitePool,
    ) {
        insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
        let (store, _) = staged_missing_task();
        let query = GetTask {
            id: TaskId::try_new("PWF-0002").unwrap(),
            output: TaskReadFormat::Path,
        };

        let gotten = super::execute(&query, &store, &pool).await.unwrap();

        assert_eq!(
            gotten,
            TaskRead::Path(TaskNotePath::new("/notes/pwf/PWF-0002.md".into()))
        );
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn get_markdown_preserves_missing_note_source_error(pool: sqlx::SqlitePool) {
        insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
        let (store, _) = staged_missing_task();
        let store = store.with_failure(ProjectNoteFailure::Read);
        let query = GetTask {
            id: TaskId::try_new("PWF-0002").unwrap(),
            output: TaskReadFormat::Markdown,
        };

        let error = super::execute(&query, &store, &pool).await.unwrap_err();

        assert_eq!(
            error.to_string(),
            "injected in-memory store failure: project-note-read"
        );
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn data_read_validates_and_types_persisted_task_fields(pool: sqlx::SqlitePool) {
        insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
        let store = InMemoryStore::default().with_project(
            "pwf",
            vec![TaskRecord {
                title: "Typed task".to_string(),
                status: TaskStatus::Done,
                completed: Some(app_date("2026-08-12")),
                commits: Some("'a..b, c..d'".to_string()),
                tags: Some(RawTaskTags::new("[rust, sqlite]")),
                effort: Some(" high ".to_string()),
                blocked_by: Some("'[[CFG-0014]]'".to_string()),
                section: Some("Human".parse().unwrap()),
                body: "\n  authored body  \n".to_string(),
                ..task_record("PWF-0001")
            }],
        );

        let read = super::execute(
            &GetTask {
                id: "PWF-0001".parse().unwrap(),
                output: TaskReadFormat::Data,
            },
            &store,
            &pool,
        )
        .await
        .unwrap();
        let TaskRead::Data(task) = read else {
            panic!("expected typed task data");
        };

        assert_eq!(task.title.as_ref(), "typed task");
        assert_eq!(task.commits.as_ref().map(AsRef::as_ref), Some("a..b, c..d"));
        assert_eq!(task.effort.as_ref().map(AsRef::as_ref), Some("high"));
        assert_eq!(
            task.blocked_by
                .as_ref()
                .map(|blocked| blocked.iter().map(AsRef::as_ref).collect::<Vec<_>>()),
            Some(vec!["CFG-0014"])
        );
        assert_eq!(task.prompt.as_ref(), "authored body");
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn data_read_reports_an_invalid_persisted_title(pool: sqlx::SqlitePool) {
        insert_project(&pool, "PWF", "pwf", "/projects/pwf", "/tasks/pwf", false).await;
        let store = InMemoryStore::default().with_project(
            "pwf",
            vec![TaskRecord {
                title: "x".repeat(201),
                ..task_record("PWF-0001")
            }],
        );

        let error = super::execute(
            &GetTask {
                id: "PWF-0001".parse().unwrap(),
                output: TaskReadFormat::Data,
            },
            &store,
            &pool,
        )
        .await
        .unwrap_err();

        assert!(matches!(
            &error,
            GetTaskError::InvalidTaskData { field: "title", .. }
        ));
        assert!(error.source().is_some());
    }
}
