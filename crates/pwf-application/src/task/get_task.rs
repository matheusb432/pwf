use pwf_models::task::{Task, TaskId};
use pwf_wire::task::TaskRecordError;

use super::get_task_record::{self, GetTaskRecordError};
use crate::ports::task_vault::TaskVault;

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
    pool: &sqlx::SqlitePool,
) -> Result<Task, GetTaskError> {
    get_task_record::execute(id, store, pool)
        .await?
        .into_task()
        .map_err(Into::into)
}
