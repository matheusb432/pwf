use pwf_models::task::TaskTimestampError;
use pwf_wire::task::{CancelTask, ClosedTaskAction, TaskMutationResult};

use super::{
    CloseTaskError,
    resolve_task_project::{self, ResolveTaskProjectError},
    task_closure::{self, TaskClosure},
};
use crate::ports::{clock::Clock, task_vault::TaskVault};

#[derive(Debug, thiserror::Error)]
pub enum CancelTaskError {
    #[error(transparent)]
    ResolveProject(#[from] ResolveTaskProjectError),
    #[error(transparent)]
    Close(#[from] CloseTaskError),
    #[error("cannot read the task cancellation time: {0}")]
    Clock(#[from] TaskTimestampError),
}

#[cqrsy::command]
pub async fn execute(
    command: &CancelTask,
    store: &impl TaskVault,
    pool: &sqlx::SqlitePool,
    clock: &impl Clock,
) -> Result<TaskMutationResult<()>, CancelTaskError> {
    let project = resolve_task_project::execute(command.id.clone(), pool).await?;
    let summary = task_closure::close(
        &TaskClosure {
            action: ClosedTaskAction::Cancelled,
            id: &command.id,
            completed_at: clock.now()?,
            report: Some(&command.report),
            commits: command.commits.as_ref(),
            expected_revision: command.expected_revision.as_ref(),
        },
        store,
        &project,
    )?;
    Ok(TaskMutationResult {
        outcome: (),
        task: Some(summary),
    })
}
