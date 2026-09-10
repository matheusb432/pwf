use pwf_models::task::{TaskId, TaskTimestampError};
use pwf_wire::task::{AddTask, AddTaskPrompt, TaskMutationResult, TaskMutationSummary};

use super::{
    TaskPromptTitleError,
    blocked_by::{self, BlockedByValidationError},
    infer_task_title,
    lane_configuration::{TaskPromptLanes, TaskPromptLanesError},
    note_body::{render, render_lanes},
    read_task_dependencies::{self, ReadTaskDependencies, ReadTaskDependenciesError},
};
use crate::{
    ports::{
        clock::Clock,
        task_vault::{NewTask, NewTaskBody, TaskVault},
    },
    project::{get_active_project, get_project::GetProjectError},
};

#[derive(Debug, thiserror::Error)]
pub enum AddTaskError {
    #[error(transparent)]
    GetProject(#[from] GetProjectError),
    #[error(transparent)]
    QueryProject(anyhow::Error),
    #[error("cannot determine the next task ID for {project}: {source}")]
    AllocateTaskId {
        project: pwf_models::project::ProjectName,
        #[source]
        source: anyhow::Error,
    },
    #[error("Unknown --blocked-by id(s): {}.", blocked_by::format_task_ids(ids))]
    UnknownBlockedByIds { ids: Vec<TaskId> },
    #[error("cannot validate --blocked-by task {id}: {source}")]
    ReadBlockedBy {
        id: TaskId,
        #[source]
        source: anyhow::Error,
    },
    #[error("task {target} cannot be blocked by itself ({blocker})")]
    SelfBlockedBy { target: TaskId, blocker: TaskId },
    #[error("blocked_by cycle: {}", blocked_by::format_task_ids_path(path))]
    BlockedByCycle { path: Vec<TaskId> },
    #[error("task {task} at {path} has malformed blocked_by metadata {raw:?}: {reason}")]
    MalformedBlockedBy {
        task: TaskId,
        path: Box<pwf_wire::task::TaskNotePath>,
        raw: Box<str>,
        reason: Box<str>,
    },
    #[error(transparent)]
    InvalidTitle(#[from] TaskPromptTitleError),
    #[error(transparent)]
    PromptLanes(#[from] TaskPromptLanesError),
    #[error("cannot read the task creation time: {0}")]
    Clock(#[from] TaskTimestampError),
    #[error(transparent)]
    WriteStore(anyhow::Error),
}

#[cqrsy::command]
pub async fn execute(
    command: AddTask,
    store: &impl TaskVault,
    pool: &sqlx::SqlitePool,
    clock: &impl Clock,
) -> Result<TaskMutationResult<TaskId>, AddTaskError> {
    let AddTask {
        project_id,
        prompt,
        blocked_by,
        effort,
        priority,
        tags,
    } = command;
    let project = get_active_project::execute(&project_id, pool).await?;
    let (title, body) = match prompt {
        AddTaskPrompt::Body { title, body } => (title, NewTaskBody::Verbatim(body)),
        AddTaskPrompt::Shorthand(prompt) => {
            let lanes = TaskPromptLanes::load(pool).await?;
            (
                infer_task_title(&prompt, &lanes)?,
                NewTaskBody::Rendered(render(&prompt, &lanes)),
            )
        }
        AddTaskPrompt::Structured { title, lanes } => {
            let configuration = TaskPromptLanes::load(pool).await?;
            (
                title,
                NewTaskBody::Rendered(render_lanes(&lanes, &configuration)),
            )
        }
    };
    let id = store
        .next_task_id(&project)
        .map_err(|source| AddTaskError::AllocateTaskId {
            project: project.title.clone(),
            source: anyhow::Error::new(source),
        })?;

    if let Some(blockers) = blocked_by.as_ref() {
        let dependencies = read_task_dependencies::execute(
            ReadTaskDependencies {
                target: &id,
                blockers,
            },
            store,
            pool,
        )
        .await
        .map_err(|error| match error {
            ReadTaskDependenciesError::ReadStore { id, source } => {
                AddTaskError::ReadBlockedBy { id, source }
            }
            ReadTaskDependenciesError::QueryProject(source) => AddTaskError::QueryProject(source),
        })?;
        blocked_by::validate(&id, blockers, blockers, &dependencies)
            .map_err(map_blocked_by_error)?;
    }
    let summary = TaskMutationSummary {
        id: id.clone(),
        title: title.to_string(),
        status: pwf_models::task::TaskStatus::Active,
    };
    store
        .insert_task(
            &project,
            &id,
            NewTask {
                body,
                title,
                created_at: clock.now()?,
                blocked_by,
                effort,
                priority,
                tags,
            },
        )
        .map_err(|source| AddTaskError::WriteStore(anyhow::Error::new(source)))?;
    Ok(TaskMutationResult {
        outcome: id,
        task: Some(summary),
    })
}

fn map_blocked_by_error(error: BlockedByValidationError) -> AddTaskError {
    match error {
        BlockedByValidationError::UnknownIds { ids } => AddTaskError::UnknownBlockedByIds { ids },
        BlockedByValidationError::SelfDependency { target, blocker } => {
            AddTaskError::SelfBlockedBy { target, blocker }
        }
        BlockedByValidationError::Cycle { path } => AddTaskError::BlockedByCycle { path },
        BlockedByValidationError::MalformedMetadata {
            task,
            path,
            raw,
            reason,
        } => AddTaskError::MalformedBlockedBy {
            task,
            path,
            raw,
            reason,
        },
    }
}
