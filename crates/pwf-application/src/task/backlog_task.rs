use pwf_models::task::{TaskId, TaskStatus};
use pwf_wire::{
    set_field::SetField,
    task::{BacklogTaskOutcome, TaskMutationResult, TaskMutationSummary},
};

use super::{
    commit_task_writes, expected_task_revision,
    get_task_record::{self, GetTaskRecordError, LoadedTaskRecord},
};
use crate::ports::task_vault::{TaskMutationError, TaskPatch, TaskVault, TaskWrite};

#[derive(Debug, thiserror::Error)]
pub enum BacklogTaskError {
    #[error("cannot backlog closed task {id}; run `pwf task activate {id}` first.")]
    ClosedTask { id: TaskId },
    #[error(transparent)]
    Read(#[from] GetTaskRecordError),
    #[error(transparent)]
    Mutation(#[from] TaskMutationError<anyhow::Error>),
}

#[cqrsy::command]
pub async fn execute(
    id: &TaskId,
    store: &impl TaskVault,
    pool: &sqlx::SqlitePool,
) -> Result<TaskMutationResult<BacklogTaskOutcome>, BacklogTaskError> {
    let LoadedTaskRecord { project, record } = get_task_record::load(id, store, pool).await?;
    if record.status.is_closed() {
        return Err(BacklogTaskError::ClosedTask { id: id.clone() });
    }
    let outcome = if record.status == TaskStatus::Backlog {
        BacklogTaskOutcome::AlreadyBacklogged
    } else {
        commit_task_writes(
            store,
            &project,
            vec![expected_task_revision(&record)],
            vec![TaskWrite::Patch {
                id: id.clone(),
                patch: TaskPatch {
                    status: SetField::Set(TaskStatus::Backlog),
                    ..TaskPatch::default()
                },
            }],
        )?;
        BacklogTaskOutcome::Backlogged
    };
    Ok(TaskMutationResult {
        outcome,
        task: Some(TaskMutationSummary {
            id: id.clone(),
            title: record.title,
            status: TaskStatus::Backlog,
        }),
    })
}
