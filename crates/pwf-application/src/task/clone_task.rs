use pwf_models::task::TaskId;
use pwf_wire::task::{AddTask, AddTaskPrompt, CloneTask, ClonedTaskProjectId, TaskMutationResult};

use crate::{
    ports::{clock::Clock, task_vault::TaskVault},
    task::{
        add_task::{self, AddTaskError},
        get_task::{self, GetTaskError},
    },
};

#[derive(Debug, thiserror::Error)]
pub enum CloneTaskError {
    #[error(transparent)]
    GetTask(#[from] GetTaskError),
    #[error(transparent)]
    AddTask(#[from] AddTaskError),
}

#[cqrsy::command]
pub async fn execute(
    command: CloneTask,
    store: &impl TaskVault,
    pool: &sqlx::SqlitePool,
    clock: &impl Clock,
) -> Result<TaskMutationResult<TaskId>, CloneTaskError> {
    let CloneTask { id, project_id } = command;
    let project_id = match project_id {
        ClonedTaskProjectId::SameAsTask => id.project_id().to_owned(),
        ClonedTaskProjectId::Id(project_id) => project_id,
    };
    let task = get_task::execute(&id, store, pool).await?;

    Ok(add_task::execute(
        AddTask {
            project_id,
            prompt: AddTaskPrompt::from_body(task.title, task.prompt),
            blocked_by: task.blocked_by,
            effort: task.effort,
            tags: task.tags,
            priority: task.priority,
        },
        store,
        pool,
        clock,
    )
    .await?)
}
