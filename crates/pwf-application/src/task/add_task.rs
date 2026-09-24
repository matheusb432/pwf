use pwf_models::task::{TaskId, TaskTimestampError};
use pwf_wire::task::{AddTask, AddTaskBody, TaskMutationResult, TaskMutationSummary};

use super::{
    TaskBodyTitleError,
    blocked_by::{self, BlockedByValidationError},
    content::render_for_creation,
    infer_task_title,
    read_task_dependencies::{self, ReadTaskDependencies, ReadTaskDependenciesError},
};
use crate::{
    ports::{
        clock::Clock,
        project_store::ProjectStore,
        task_vault::{NewTask, NewTaskBody, TaskInsertion, TaskVault},
        user_settings::{TaskBodyPresetReader, UserSettingsLoadError},
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
        path: Box<pwf_wire::task::TaskFilePath>,
        raw: Box<str>,
        reason: Box<str>,
    },
    #[error(transparent)]
    InvalidTitle(#[from] TaskBodyTitleError),
    #[error(transparent)]
    TaskBodyPresets(#[from] UserSettingsLoadError),
    #[error("cannot read the task creation time: {0}")]
    Clock(#[from] TaskTimestampError),
    #[error(transparent)]
    WriteStore(anyhow::Error),
}

#[cqrsy::command]
pub async fn execute(
    command: AddTask,
    store: &impl TaskVault,
    project_store: &impl ProjectStore,
    clock: &impl Clock,
    preset_reader: &impl TaskBodyPresetReader,
) -> Result<TaskMutationResult<TaskId>, AddTaskError> {
    let AddTask {
        project_id,
        body,
        blocked_by,
        effort,
        priority,
        tags,
    } = command;
    let project = get_active_project::execute(&project_id, project_store).await?;
    let (title, body) = match body {
        AddTaskBody::Body { title, body } => (title, NewTaskBody::Verbatim(body)),
        AddTaskBody::Shorthand(body) => {
            let presets = preset_reader.load_task_body_presets()?;
            let preset = presets.for_project(&project.id);
            (
                infer_task_title(&body, preset)?,
                NewTaskBody::Rendered(render_for_creation(&body, preset)),
            )
        }
    };
    let id = allocate_task_id(&project, store, project_store)
        .await
        .map_err(|source| AddTaskError::AllocateTaskId {
            project: project.title.clone(),
            source,
        })?;

    if let Some(blockers) = blocked_by.as_ref() {
        let dependencies = read_task_dependencies::execute(
            ReadTaskDependencies {
                target: &id,
                blockers,
            },
            store,
            project_store,
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
        .insert_task(TaskInsertion::new(
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
        ))
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

async fn allocate_task_id(
    project: &pwf_models::project::Project,
    tasks: &impl TaskVault,
    projects: &impl ProjectStore,
) -> Result<TaskId, anyhow::Error> {
    if let Some(id) = projects.reserve_task_id(&project.id).await? {
        return Ok(id);
    }
    let highest = tasks.highest_task_id(project)?;
    projects
        .advance_task_sequence(&project.id, highest.as_ref())
        .await?;
    projects
        .reserve_task_id(&project.id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("task sequence was not initialized for {}", project.id))
}
