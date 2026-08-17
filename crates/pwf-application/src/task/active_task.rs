//! Resolves one active task with its persisted record and launch view.

use pwf_models::{
    project::Project,
    task::{TaskId, TaskStatus},
};
use pwf_wire::task::{ResolveTaskProject, TaskView};

use crate::{
    ports::task_record::{TaskRecord, TaskStore},
    task::{
        resolve_task_project::{self, ResolveTaskProjectError},
        task_view,
    },
};

#[derive(Debug)]
pub(in crate::task) struct FoundActiveTask {
    pub(in crate::task) project: Project,
    pub(in crate::task) record: TaskRecord,
    pub(in crate::task) task: TaskView,
}

#[derive(Debug, thiserror::Error)]
pub(in crate::task) enum FindActiveTaskError {
    #[error("Active task not found: {id}")]
    TaskNotFound { id: TaskId },
    #[error("Task id is ambiguous: {id}")]
    AmbiguousId { id: TaskId },
    #[error(transparent)]
    ResolveProject(#[from] ResolveTaskProjectError),
    #[error("{0}")]
    ReadStore(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error(transparent)]
    InvalidTaskView(#[from] task_view::TaskViewError),
}

pub(in crate::task) async fn find(
    id: &TaskId,
    store: &impl TaskStore,
    pool: &sqlx::SqlitePool,
) -> Result<FoundActiveTask, FindActiveTaskError> {
    let project =
        resolve_task_project::execute(ResolveTaskProject { id: id.clone() }, pool).await?;
    let (record, task) = find_active_task(store, &project, id)?;
    Ok(FoundActiveTask {
        project,
        record,
        task,
    })
}

fn find_active_task(
    store: &impl TaskStore,
    project: &Project,
    task_id: &TaskId,
) -> Result<(TaskRecord, TaskView), FindActiveTaskError> {
    let records = store
        .list(project)
        .map_err(|error| FindActiveTaskError::ReadStore(Box::new(error)))?;
    let mut matched = records
        .into_iter()
        .filter(|record| record.status == TaskStatus::Active && record.id == *task_id);
    let Some(record) = matched.next() else {
        return Err(FindActiveTaskError::TaskNotFound {
            id: task_id.clone(),
        });
    };
    if matched.next().is_some() {
        return Err(FindActiveTaskError::AmbiguousId {
            id: task_id.clone(),
        });
    }
    let task =
        task_view::enrich(&record, project.source.value())?.into_task_view(project.title.clone());
    Ok((record, task))
}

#[cfg(test)]
mod tests {
    use std::{assert_matches, num::NonZeroUsize};

    use pwf_models::{
        project::Project,
        task::{TaskId, TaskStatus},
    };
    use pwf_wire::task::{TaskIndexPath, TaskNotePath};

    use super::{FindActiveTaskError, TaskView, find_active_task};
    use crate::{
        ports::task_record::{IndexPlacement, TaskRecord},
        testing::{InMemoryStore, app_date, project, task_record},
    };

    fn record(id: &str) -> TaskRecord {
        TaskRecord {
            title: format!("title {id}"),
            created: Some(app_date("2026-07-07")),
            body: "do the thing".to_string(),
            source: String::new(),
            locator: TaskNotePath::new(format!("/notes/pwf/{id}.md").into()),
            placement: Some(IndexPlacement {
                index_path: TaskIndexPath::new("/notes/pwf/pwf.md".into()),
                line: NonZeroUsize::new(7).unwrap(),
            }),
            ..task_record(id)
        }
    }

    fn find(
        store: &InMemoryStore,
        project: &Project,
        id: &str,
    ) -> Result<TaskView, FindActiveTaskError> {
        find_active_task(store, project, &TaskId::try_new(id).unwrap()).map(|(_, task)| task)
    }

    #[test]
    fn finds_open_item_enriched_with_launchability() {
        let store = InMemoryStore::default().with_project("pwf", vec![record("PWF-0001")]);
        let project = project("PWF", "pwf");

        let task = find(&store, &project, "PWF-0001").unwrap();

        assert_eq!(task.id.as_ref(), "PWF-0001");
        assert_eq!(task.project.as_ref(), "pwf");
        assert_eq!(task.project_path.as_ref(), "/work/pwf");
        assert_eq!(task.prompt.as_ref(), "do the thing");
        assert!(task.launch.is_ready());
    }

    #[test]
    fn missing_task_reports_the_typed_id() {
        let store = InMemoryStore::default().with_project("pwf", vec![record("PWF-0001")]);
        let project = project("PWF", "pwf");

        let error = find(&store, &project, "PWF-9999").unwrap_err();

        assert!(matches!(
            error,
            FindActiveTaskError::TaskNotFound { ref id } if id.as_ref() == "PWF-9999"
        ));
        assert_eq!(error.to_string(), "Active task not found: PWF-9999");
    }

    #[test]
    fn duplicate_link_in_one_project_is_ambiguous() {
        let store = InMemoryStore::default()
            .with_project("pwf", vec![record("PWF-0001"), record("PWF-0001")]);
        let project = project("PWF", "pwf");

        let error = find(&store, &project, "PWF-0001").unwrap_err();

        assert!(matches!(error, FindActiveTaskError::AmbiguousId { .. }));
    }

    #[test]
    fn find_accepts_unlinked_active_and_rejects_closed_records() {
        let done = TaskRecord {
            status: TaskStatus::Done,
            placement: None,
            ..record("PWF-0001")
        };
        let unlinked_active = TaskRecord {
            placement: None,
            ..record("PWF-0002")
        };
        let store = InMemoryStore::default().with_project("pwf", vec![done, unlinked_active]);
        let project = project("PWF", "pwf");

        assert_eq!(
            find(&store, &project, "PWF-0002").unwrap().id.as_ref(),
            "PWF-0002"
        );
        assert_matches!(find(&store, &project, "PWF-0001"), Err(
            FindActiveTaskError::TaskNotFound { id }
        ) if id.as_ref() == "PWF-0001");
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn unknown_project_id_preserves_the_resolution_error(pool: sqlx::SqlitePool) {
        let id = "XYZ-0001".parse().unwrap();

        let error = super::find(&id, &InMemoryStore::default(), &pool)
            .await
            .unwrap_err();

        assert!(matches!(error, FindActiveTaskError::ResolveProject(_)));
        assert_eq!(
            error.to_string(),
            "Unknown project ID `XYZ` for task XYZ-0001"
        );
    }
}
