use pwf_models::task::TaskTimestampError;
use pwf_wire::task::{ClosedTaskAction, CompleteTask, TaskMutationResult};

use super::{
    CloseTaskError,
    resolve_task_project::{self, ResolveTaskProjectError},
    task_closure::{self, TaskClosure},
};
use crate::ports::{clock::Clock, task_vault::TaskVault};

#[derive(Debug, thiserror::Error)]
pub enum CompleteTaskError {
    #[error(transparent)]
    ResolveProject(#[from] ResolveTaskProjectError),
    #[error(transparent)]
    Close(#[from] CloseTaskError),
    #[error("cannot read the task completion time: {0}")]
    Clock(#[from] TaskTimestampError),
}

#[cqrsy::command]
pub async fn execute(
    command: &CompleteTask,
    store: &impl TaskVault,
    pool: &sqlx::SqlitePool,
    clock: &impl Clock,
) -> Result<TaskMutationResult<()>, CompleteTaskError> {
    let project = resolve_task_project::execute(command.id.clone(), pool).await?;
    let summary = task_closure::close(
        &TaskClosure {
            action: ClosedTaskAction::Done,
            id: &command.id,
            completed_at: clock.now()?,
            report: command.report.as_ref(),
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
