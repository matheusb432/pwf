//! Resolves one active record and its display heading.

use pwf_models::{
    project::Project,
    task::{TaskId, TaskStatus},
};
use pwf_wire::task::{TaskHeading, TaskRecord};

use crate::{
    ports::task_vault::TaskVault,
    task::{
        resolve_task_project::{self, ResolveTaskProjectError},
        task_projection,
    },
};

#[derive(Debug)]
pub(in crate::task) struct FoundActiveTask {
    pub(in crate::task) project: Project,
    pub(in crate::task) record: TaskRecord,
    pub(in crate::task) heading: TaskHeading,
}

#[derive(Debug, thiserror::Error)]
pub(in crate::task) enum FindActiveTaskError {
    #[error("Active task not found: {id}")]
    TaskNotFound { id: TaskId },
    #[error("Task id is ambiguous: {id}")]
    AmbiguousId { id: TaskId },
    #[error(transparent)]
    ResolveProject(#[from] ResolveTaskProjectError),
    #[error(transparent)]
    ReadStore(anyhow::Error),
    #[error(transparent)]
    InvalidTaskProjection(#[from] task_projection::TaskProjectionError),
}

pub(in crate::task) async fn find(
    id: &TaskId,
    store: &impl TaskVault,
    pool: &sqlx::SqlitePool,
) -> Result<FoundActiveTask, FindActiveTaskError> {
    let project = resolve_task_project::execute(id.clone(), pool).await?;
    let (record, heading) = find_active_task(store, &project, id)?;
    Ok(FoundActiveTask {
        project,
        record,
        heading,
    })
}

fn find_active_task(
    store: &impl TaskVault,
    project: &Project,
    task_id: &TaskId,
) -> Result<(TaskRecord, TaskHeading), FindActiveTaskError> {
    let records = store
        .list_tasks(project)
        .map_err(|error| FindActiveTaskError::ReadStore(anyhow::Error::new(error)))?;
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
    let heading = task_projection::task_heading(&record.id, &record.title)?;
    task_projection::task_effort(&record.id, record.effort.as_deref())?;
    task_projection::task_priority(&record.id, record.priority.as_deref())?;
    Ok((record, heading))
}
