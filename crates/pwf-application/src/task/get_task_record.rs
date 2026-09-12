use pwf_models::{project::Project, task::TaskId};
use pwf_wire::task::TaskRecord;

use crate::{
    ports::task_vault::TaskVault,
    project::{get_active_project, get_project::GetProjectError},
};

#[derive(Debug, thiserror::Error)]
pub enum GetTaskRecordError {
    #[error("Task not found: {id}")]
    TaskNotFound { id: TaskId },
    #[error(transparent)]
    ReadStore(anyhow::Error),
    #[error(transparent)]
    QueryProject(anyhow::Error),
}

#[cqrsy::query]
pub async fn execute(
    id: &TaskId,
    store: &impl TaskVault,
    pool: &sqlx::SqlitePool,
) -> Result<TaskRecord, GetTaskRecordError> {
    load(id, store, pool).await.map(|loaded| loaded.record)
}

pub(super) struct LoadedTaskRecord {
    pub(super) project: Project,
    pub(super) record: TaskRecord,
}

pub(super) async fn load(
    id: &TaskId,
    store: &impl TaskVault,
    pool: &sqlx::SqlitePool,
) -> Result<LoadedTaskRecord, GetTaskRecordError> {
    let project = match get_active_project::execute(id.project_id(), pool).await {
        Ok(project) => project,
        Err(GetProjectError::ProjectNotFound { .. }) => {
            return Err(GetTaskRecordError::TaskNotFound { id: id.clone() });
        }
        Err(error) => return Err(GetTaskRecordError::QueryProject(anyhow::Error::new(error))),
    };
    let record = store
        .get_task_record(&project, id)
        .map_err(|error| GetTaskRecordError::ReadStore(anyhow::Error::new(error)))?
        .ok_or_else(|| GetTaskRecordError::TaskNotFound { id: id.clone() })?;
    Ok(LoadedTaskRecord { project, record })
}
