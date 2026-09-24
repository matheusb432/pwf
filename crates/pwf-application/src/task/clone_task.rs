use pwf_models::task::TaskId;
use pwf_wire::task::{AddTask, AddTaskBody, CloneTask, ClonedTaskProjectId, TaskMutationResult};

use crate::{
    ports::{
        clock::Clock, project_store::ProjectStore, task_vault::TaskVault,
        user_settings::TaskBodyPresetReader,
    },
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
    project_store: &impl ProjectStore,
    clock: &impl Clock,
    preset_reader: &impl TaskBodyPresetReader,
) -> Result<TaskMutationResult<TaskId>, CloneTaskError> {
    let CloneTask { id, project_id } = command;
    let project_id = match project_id {
        ClonedTaskProjectId::SameAsTask => id.project_id().to_owned(),
        ClonedTaskProjectId::Id(project_id) => project_id,
    };
    let task = get_task::execute(&id, store, project_store).await?;

    Ok(add_task::execute(
        AddTask {
            project_id,
            body: AddTaskBody::from_body(task.title, task.body),
            blocked_by: task.blocked_by,
            effort: task.effort,
            tags: task.tags,
            priority: task.priority,
        },
        store,
        project_store,
        clock,
        preset_reader,
    )
    .await?)
}
