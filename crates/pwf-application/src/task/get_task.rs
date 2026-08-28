use pwf_models::{
    project::ProjectName,
    task::{CommitRanges, EffortTier, TaskId, TaskPrompt, TaskTitle},
};
use pwf_wire::task::{GetTask, TaskData, TaskRead, TaskReadFormat};

use crate::{
    ports::{
        project_note::ProjectNoteStore,
        task_record::{Materialization, StoredBlockedBy, TaskRecord, TaskStore},
    },
    project::{get_active_project, get_project::GetProjectError},
    task::tags,
};

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum GetTaskError {
    #[error("Task not found: {id}")]
    TaskNotFound { id: TaskId },
    #[error(transparent)]
    ReadStore(anyhow::Error),
    #[error(transparent)]
    ReadMarkdown(anyhow::Error),
    #[error(transparent)]
    QueryProject(anyhow::Error),
    #[error("Invalid task {field}: {source}")]
    InvalidTaskData {
        field: &'static str,
        #[source]
        source: anyhow::Error,
    },
    #[error("task {id} at {path} has malformed blocked_by metadata {raw:?}: {reason}")]
    MalformedBlockedBy {
        id: TaskId,
        path: Box<pwf_wire::task::TaskNotePath>,
        raw: Box<str>,
        reason: Box<str>,
    },
}

/// Returns a task's selected representation.
#[cqrsy::query]
pub async fn execute(
    query: &GetTask,
    store: &(impl TaskStore + ProjectNoteStore),
    pool: &sqlx::SqlitePool,
) -> Result<TaskRead, GetTaskError> {
    let project = match get_active_project::execute(query.id.project_id().clone(), pool).await {
        Ok(project) => project,
        Err(GetProjectError::ProjectNotFound { .. }) => {
            return Err(GetTaskError::TaskNotFound {
                id: query.id.clone(),
            });
        }
        Err(error) => return Err(GetTaskError::QueryProject(anyhow::Error::new(error))),
    };
    let record = store
        .get(&project, &query.id)
        .map_err(|error| GetTaskError::ReadStore(anyhow::Error::new(error)))?
        .ok_or_else(|| GetTaskError::TaskNotFound {
            id: query.id.clone(),
        })?;
    match query.output {
        TaskReadFormat::Path => Ok(TaskRead::Path(record.locator)),
        TaskReadFormat::Markdown
            if matches!(record.materialization, Materialization::MissingNote { .. }) =>
        {
            store
                .read_note_markdown(&record.locator)
                .map(TaskRead::Markdown)
                .map_err(|error| GetTaskError::ReadMarkdown(anyhow::Error::new(error)))
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
    let blocked_by = match &record.blocked_by {
        StoredBlockedBy::Absent => None,
        StoredBlockedBy::Valid(blocked_by) => Some(blocked_by.clone()),
        StoredBlockedBy::Malformed { raw, reason } => {
            return Err(GetTaskError::MalformedBlockedBy {
                id: record.id.clone(),
                path: Box::new(record.locator.clone()),
                raw: raw.clone().into_boxed_str(),
                reason: reason.clone().into_boxed_str(),
            });
        }
    };
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
        source: anyhow::Error::new(source),
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use pwf_models::task::TaskStatus;
    use pwf_wire::task::{RawTaskTags, TaskNotePath, TaskRead, TaskReadFormat};

    use super::{GetTask, GetTaskError, TaskId};
    use crate::{
        ports::task_record::{StoredBlockedBy, TaskRecord},
        task::get_task,
        testing::{
            FOO_0001_SOURCE, InMemoryStore, ProjectNoteFailure, app_date, insert_project,
            staged_missing_task, staged_task, stored_blocked_by, task_record,
        },
    };

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn get_streams_source_verbatim(pool: sqlx::SqlitePool) {
        insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
        let (store, _) = staged_task();
        let query = GetTask {
            id: TaskId::try_new("FOO-0001").unwrap(),
            output: TaskReadFormat::Markdown,
        };

        let gotten = get_task::execute(&query, &store, &pool).await.unwrap();

        assert_eq!(gotten, TaskRead::Markdown(FOO_0001_SOURCE.to_string()));
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn get_path_returns_missing_note_locator_without_reading_markdown(
        pool: sqlx::SqlitePool,
    ) {
        insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
        let (store, _) = staged_missing_task();
        let query = GetTask {
            id: TaskId::try_new("FOO-0002").unwrap(),
            output: TaskReadFormat::Path,
        };

        let gotten = get_task::execute(&query, &store, &pool).await.unwrap();

        assert_eq!(
            gotten,
            TaskRead::Path(TaskNotePath::new("/notes/foo/FOO-0002.md".into()))
        );
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn get_markdown_preserves_missing_note_source_error(pool: sqlx::SqlitePool) {
        insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
        let (store, _) = staged_missing_task();
        let store = store.with_failure(ProjectNoteFailure::Read);
        let query = GetTask {
            id: TaskId::try_new("FOO-0002").unwrap(),
            output: TaskReadFormat::Markdown,
        };

        let error = get_task::execute(&query, &store, &pool).await.unwrap_err();

        assert_eq!(
            error.to_string(),
            "injected in-memory store failure: project-note-read"
        );
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn data_read_validates_and_types_persisted_task_fields(pool: sqlx::SqlitePool) {
        insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
        let store = InMemoryStore::default().with_project(
            "foo",
            vec![TaskRecord {
                title: "Typed task".to_string(),
                status: TaskStatus::Done,
                completed: Some(app_date("2026-08-12")),
                commits: Some("'a..b, c..d'".to_string()),
                tags: Some(RawTaskTags::new("[rust, sqlite]")),
                effort: Some(" high ".to_string()),
                blocked_by: stored_blocked_by(&["AUX-0014"]),
                section: Some("Human".parse().unwrap()),
                body: "\n  authored body  \n".to_string(),
                ..task_record("FOO-0001")
            }],
        );

        let read = get_task::execute(
            &GetTask {
                id: "FOO-0001".parse().unwrap(),
                output: TaskReadFormat::Data,
            },
            &store,
            &pool,
        )
        .await
        .unwrap();
        let task = match read {
            TaskRead::Data(task) => Some(task),
            _ => None,
        };
        assert!(task.is_some());
        let task = task.unwrap();

        assert_eq!(task.title.as_ref(), "typed task");
        assert_eq!(task.commits.as_ref().map(AsRef::as_ref), Some("a..b, c..d"));
        assert_eq!(task.effort.as_ref().map(AsRef::as_ref), Some("high"));
        assert_eq!(
            task.blocked_by
                .as_ref()
                .map(|blocked| blocked.iter().map(AsRef::as_ref).collect::<Vec<_>>()),
            Some(vec!["AUX-0014"])
        );
        assert_eq!(task.prompt.as_ref(), "authored body");
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn data_read_reports_an_invalid_persisted_title(pool: sqlx::SqlitePool) {
        insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
        let store = InMemoryStore::default().with_project(
            "foo",
            vec![TaskRecord {
                title: "x".repeat(201),
                ..task_record("FOO-0001")
            }],
        );

        let error = get_task::execute(
            &GetTask {
                id: "FOO-0001".parse().unwrap(),
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

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn data_read_reports_malformed_blocked_by_with_task_path_and_raw_value(
        pool: sqlx::SqlitePool,
    ) {
        insert_project(&pool, "FOO", "foo", "/projects/foo", "/tasks/foo", false).await;
        let store = InMemoryStore::default().with_project(
            "foo",
            vec![TaskRecord {
                blocked_by: StoredBlockedBy::Malformed {
                    raw: "\"[[AUX-0001]]\"".to_string(),
                    reason: "expected a sequence".to_string(),
                },
                ..task_record("FOO-0001")
            }],
        );

        let error = get_task::execute(
            &GetTask {
                id: "FOO-0001".parse().unwrap(),
                output: TaskReadFormat::Data,
            },
            &store,
            &pool,
        )
        .await
        .unwrap_err();

        assert!(matches!(
            error,
            GetTaskError::MalformedBlockedBy { ref id, ref path, ref raw, .. }
                if id.as_ref() == "FOO-0001"
                    && path.as_path() == std::path::Path::new("/mem/foo-bar/FOO-0001.md")
                    && raw.as_ref() == "\"[[AUX-0001]]\""
        ));
    }
}
