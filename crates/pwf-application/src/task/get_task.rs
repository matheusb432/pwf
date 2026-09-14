use pwf_models::task::{Task, TaskId};
use pwf_wire::task::TaskRecordError;

use super::get_task_record::{self, GetTaskRecordError};
use crate::ports::{project_store::ProjectStore, task_vault::TaskVault};

#[derive(Debug, thiserror::Error)]
pub enum GetTaskError {
    #[error(transparent)]
    Read(#[from] GetTaskRecordError),
    #[error(transparent)]
    Parse(#[from] TaskRecordError),
}

#[cqrsy::query]
pub async fn execute(
    id: &TaskId,
    store: &impl TaskVault,
    project_store: &impl ProjectStore,
) -> Result<Task, GetTaskError> {
    get_task_record::execute(id, store, project_store)
        .await?
        .into_task()
        .map_err(Into::into)
}
