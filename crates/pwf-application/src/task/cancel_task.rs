use pwf_models::task::TaskTimestampError;
use pwf_wire::task::{CancelTask, ClosedTaskAction, TaskMutationResult};

use super::{
    CloseTaskError,
    mutation_request::{self, MutationOperation, MutationRequestState, MutationStart},
    resolve_task_project::{self, ResolveTaskProjectError},
    task_closure::{self, TaskClosure},
};
use crate::ports::{
    clock::Clock,
    task_vault::{TaskMutationError, TaskVault},
};

#[derive(Debug, thiserror::Error)]
pub enum CancelTaskError {
    #[error(transparent)]
    ResolveProject(#[from] ResolveTaskProjectError),
    #[error(transparent)]
    Close(#[from] CloseTaskError),
    #[error("cannot read the task cancellation time: {0}")]
    Clock(#[from] TaskTimestampError),
    #[error(transparent)]
    MutationRequest(#[from] mutation_request::MutationRequestError),
}

#[cqrsy::command]
pub async fn execute(
    command: &CancelTask,
    store: &impl TaskVault,
    pool: &sqlx::SqlitePool,
    clock: &impl Clock,
) -> Result<TaskMutationResult<()>, CancelTaskError> {
    let identity = mutation_request::identity(
        command.request_id.as_ref(),
        command.request_fingerprint.as_ref(),
    )?;
    if let Some(identity) = identity.as_ref()
        && let Some(replay) =
            mutation_request::find(pool, identity, MutationOperation::Cancel).await?
    {
        return match replay.state {
            MutationRequestState::Completed => Ok(TaskMutationResult {
                outcome: (),
                task: replay.task,
            }),
            MutationRequestState::Pending => Err(identity.incomplete().into()),
        };
    }
    let project = resolve_task_project::execute(command.id.clone(), pool).await?;
    let completed_at = clock.now()?;
    if let Some(identity) = identity.as_ref()
        && let MutationStart::Existing(replay) =
            mutation_request::start(pool, identity, MutationOperation::Cancel, &command.id).await?
    {
        return match replay.state {
            MutationRequestState::Completed => Ok(TaskMutationResult {
                outcome: (),
                task: replay.task,
            }),
            MutationRequestState::Pending => Err(identity.incomplete().into()),
        };
    }
    let result = task_closure::close(
        &TaskClosure {
            action: ClosedTaskAction::Cancelled,
            id: &command.id,
            completed_at,
            report: Some(&command.report),
            commits: command.commits.as_ref(),
            expected_revision: command.expected_revision.as_ref(),
        },
        store,
        &project,
    );
    let summary = match result {
        Ok(summary) => summary,
        Err(error) => {
            if let Some(identity) = identity.as_ref()
                && close_failed_before_mutation(&error)
            {
                mutation_request::discard(pool, identity, MutationOperation::Cancel).await?;
            }
            return Err(error.into());
        }
    };
    if let Some(identity) = identity.as_ref() {
        mutation_request::complete_with_task(
            pool,
            identity,
            MutationOperation::Cancel,
            "cancelled",
            &summary,
        )
        .await?;
    }
    Ok(TaskMutationResult {
        outcome: (),
        task: Some(summary),
    })
}

fn close_failed_before_mutation(error: &CloseTaskError) -> bool {
    matches!(
        error,
        CloseTaskError::TaskNotFound { .. }
            | CloseTaskError::UnknownProjectId { .. }
            | CloseTaskError::Revision(_)
            | CloseTaskError::Mutation(
                TaskMutationError::StaleTask { .. } | TaskMutationError::SourceChanged
            )
    )
}
