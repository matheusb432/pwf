use pwf_models::task::{TaskId, TaskTimestampError, TaskTitle};
use pwf_wire::task::{
    AddTask, AddTaskPrompt, AddTaskPromptKind, Materialization, StoredBlockedBy,
    TaskMutationResult, TaskMutationSummary, TaskRecord,
};

pub use super::task_creation::CreateTaskError;
use super::{
    TaskPromptTitleError,
    blocked_by::{self, BlockedByValidationError},
    infer_task_title,
    lane_configuration::{TaskPromptLanes, TaskPromptLanesError},
    mutation_request::{self, MutationOperation, MutationRequestState, MutationStart},
    note_body::{render, render_lanes},
    read_task_dependencies::{self, ReadTaskDependencies, ReadTaskDependenciesError},
    task_creation::{self, TaskCreation},
};
use crate::{
    ports::{
        clock::Clock,
        task_vault::{NewTask, TaskVault},
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
    #[error("reserved task {id} no longer matches its create request")]
    ReservedTaskChanged { id: TaskId },
    #[error("cannot inspect reserved task {id}: {source}")]
    ReadReservedTask {
        id: TaskId,
        #[source]
        source: anyhow::Error,
    },
    #[error(transparent)]
    MutationRequest(#[from] mutation_request::MutationRequestError),
    #[error("cannot read the task creation time: {0}")]
    Clock(#[from] TaskTimestampError),
    #[error(transparent)]
    WriteStore(#[from] CreateTaskError),
}

/// Creates a task record and its open index entry for a mapped project.
#[cqrsy::command]
#[expect(
    clippy::too_many_lines,
    reason = "keep receipt handling, dependency reads, and writes visible in the owning interactor"
)]
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
        request_id,
        request_fingerprint,
    } = command;
    let identity = mutation_request::identity(request_id.as_ref(), request_fingerprint.as_ref())?;
    let mut replay = match identity.as_ref() {
        Some(identity) => mutation_request::find(pool, identity, MutationOperation::Create).await?,
        None => None,
    };
    if let Some(replay) = replay.take_if(|replay| replay.state == MutationRequestState::Completed) {
        return Ok(TaskMutationResult {
            outcome: replay.task_id,
            task: replay.task,
        });
    }
    let project = get_active_project::execute(&project_id, pool).await?;
    let lane_configuration = TaskPromptLanes::load(pool).await?;
    let (title, body) = prepare_source(prompt, &lane_configuration)?;
    let replay_pending = replay.is_some();
    let id = match replay {
        Some(replay) => replay.task_id,
        None => store
            .next_task_id(&project)
            .map_err(|source| AddTaskError::AllocateTaskId {
                project: project.title.clone(),
                source: anyhow::Error::new(source),
            })?,
    };

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
    if !replay_pending
        && let Some(identity) = identity.as_ref()
        && let MutationStart::Existing(existing) =
            mutation_request::start(pool, identity, MutationOperation::Create, &id).await?
    {
        return match existing.state {
            MutationRequestState::Completed => Ok(TaskMutationResult {
                outcome: existing.task_id,
                task: existing.task,
            }),
            MutationRequestState::Pending => Err(identity.incomplete().into()),
        };
    }
    let summary = TaskMutationSummary {
        id: id.clone(),
        title: title.to_string(),
        status: pwf_models::task::TaskStatus::Active,
    };
    let existing = if replay_pending {
        store
            .get_task_record(&project, &id)
            .map_err(|source| AddTaskError::ReadReservedTask {
                id: id.clone(),
                source: anyhow::Error::new(source),
            })?
    } else {
        None
    };
    if let Some(record) = existing {
        if !created_record_matches(
            &record,
            &title,
            &body,
            blocked_by.as_ref(),
            effort,
            priority,
            tags.as_ref(),
        ) {
            return Err(AddTaskError::ReservedTaskChanged { id });
        }
        task_creation::ensure_index(store, &project, &id)?;
    } else {
        task_creation::create(
            TaskCreation {
                project: &project,
                id: &id,
                new: NewTask {
                    body,
                    title,
                    created_at: clock.now()?,
                    blocked_by,
                    effort,
                    priority,
                    tags,
                },
            },
            store,
        )?;
    }
    if let Some(identity) = identity.as_ref() {
        mutation_request::complete_with_task(
            pool,
            identity,
            MutationOperation::Create,
            "created",
            &summary,
        )
        .await?;
    }
    Ok(TaskMutationResult {
        outcome: id,
        task: Some(summary),
    })
}

fn created_record_matches(
    record: &TaskRecord,
    title: &TaskTitle,
    body: &str,
    blocked_by: Option<&pwf_models::task::BlockedBy>,
    effort: Option<pwf_models::task::EffortTier>,
    priority: Option<pwf_models::task::PriorityTier>,
    tags: Option<&pwf_models::task::TaskTags>,
) -> bool {
    let stored_blocked_by = match &record.blocked_by {
        StoredBlockedBy::Absent => None,
        StoredBlockedBy::Valid(value) => Some(value),
        StoredBlockedBy::Malformed { .. } => return false,
    };
    let stored_tags = record
        .tags
        .as_ref()
        .map(|raw| pwf_models::task::TaskTags::parse_frontmatter(raw.as_ref()))
        .transpose();
    record.status == pwf_models::task::TaskStatus::Active
        && matches!(record.materialization, Materialization::NoteFile)
        && record.title == title.as_ref()
        && record.body == body
        && stored_blocked_by == blocked_by
        && record.effort.as_deref() == effort.as_ref().map(AsRef::as_ref)
        && record.priority.as_deref() == priority.as_ref().map(AsRef::as_ref)
        && stored_tags.is_ok_and(|stored| stored.as_ref() == tags)
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

fn prepare_source(
    prompt: AddTaskPrompt,
    lane_configuration: &TaskPromptLanes,
) -> Result<(TaskTitle, String), AddTaskError> {
    match prompt.into_kind() {
        AddTaskPromptKind::Shorthand(prompt) => Ok((
            infer_task_title(&prompt, lane_configuration)?,
            render(&prompt, lane_configuration),
        )),
        AddTaskPromptKind::Structured { title, lanes } => {
            Ok((title, render_lanes(&lanes, lane_configuration)))
        }
    }
}

#[cfg(test)]
mod tests;
