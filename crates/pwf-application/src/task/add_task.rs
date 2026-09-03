use pwf_models::{
    project::Project,
    task::{TaskId, TaskTimestampError, TaskTitle},
};
use pwf_wire::{
    project::{ProjectStatusFilter, ResolveProject},
    task::{AddTask, AddTaskPromptKind},
};

pub use super::task_creation::CreateTaskError;
use super::{
    TaskPromptTitleError,
    blocked_by::{self, BlockedByValidationError},
    infer_task_title,
    lane_configuration::{TaskPromptLanes, TaskPromptLanesError},
    mutation_request::{self, MutationOperation, MutationRequestState, MutationStart},
    note_body::{render, render_lanes},
    task_creation::{self, TaskCreation},
};
use crate::{
    ports::{
        clock::Clock,
        task_vault::{Materialization, NewTask, StoredBlockedBy, TaskRecord, TaskVault},
    },
    project::{
        list_projects,
        resolve_project::{self, ResolveProjectError},
    },
};

#[derive(Debug, thiserror::Error)]
pub enum AddTaskError {
    #[error(transparent)]
    ProjectResolution(#[from] ResolveProjectError),
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
pub async fn execute(
    cmd: &AddTask,
    store: &impl TaskVault,
    pool: &sqlx::SqlitePool,
    clock: &impl Clock,
) -> Result<TaskId, AddTaskError> {
    add(cmd, store, pool, clock).await
}

async fn add(
    cmd: &AddTask,
    store: &impl TaskVault,
    pool: &sqlx::SqlitePool,
    clock: &impl Clock,
) -> Result<TaskId, AddTaskError> {
    let identity =
        mutation_request::identity(cmd.request_id.as_ref(), cmd.request_fingerprint.as_ref())?;
    let replay = match identity.as_ref() {
        Some(identity) => mutation_request::find(pool, identity, MutationOperation::Create).await?,
        None => None,
    };
    if let Some(replay) = replay
        .as_ref()
        .filter(|replay| replay.state == MutationRequestState::Completed)
    {
        return Ok(replay.task_id.clone());
    }
    let project = resolve_project::execute(
        ResolveProject {
            selector: cmd.project_selector.clone(),
            status: ProjectStatusFilter::ActiveOnly,
        },
        pool,
    )
    .await?;
    let lane_configuration = TaskPromptLanes::load(pool).await?;
    let prepared = prepare_source(cmd, &project, &lane_configuration)?;
    let id = match replay.as_ref() {
        Some(replay) => replay.task_id.clone(),
        None => store
            .next_task_id(&project)
            .map_err(|source| AddTaskError::AllocateTaskId {
                project: project.title.clone(),
                source: anyhow::Error::new(source),
            })?,
    };

    let blocked_by = match cmd.blocked_by.as_ref() {
        Some(blocked_by) => {
            let projects = list_projects::execute(ProjectStatusFilter::IncludingPaused, pool)
                .await
                .map_err(|error| AddTaskError::QueryProject(anyhow::Error::new(error)))?;
            Some(
                blocked_by::validate_and_merge(&id, None, blocked_by, store, &projects)
                    .map_err(map_blocked_by_error)?,
            )
        }
        None => None,
    };
    if replay.is_none()
        && let Some(identity) = identity.as_ref()
        && let MutationStart::Existing(existing) =
            mutation_request::start(pool, identity, MutationOperation::Create, &id).await?
    {
        return match existing.state {
            MutationRequestState::Completed => Ok(existing.task_id),
            MutationRequestState::Pending => Err(identity.incomplete().into()),
        };
    }

    let existing = if replay.is_some() {
        read_reserved_task(store, &project, &id)?
    } else {
        None
    };
    if let Some(record) = existing {
        if !created_record_matches(&record, &prepared, blocked_by.as_ref(), cmd) {
            return Err(AddTaskError::ReservedTaskChanged { id });
        }
        task_creation::ensure_index(store, &project, &id)?;
    } else {
        task_creation::create(
            TaskCreation {
                project: &prepared.project,
                id: &id,
                new: NewTask {
                    body: prepared.body,
                    title: prepared.title,
                    created_at: clock.now()?,
                    blocked_by,
                    effort: cmd.effort,
                    priority: cmd.priority,
                    tags: cmd.tags.clone(),
                },
            },
            store,
        )?;
    }
    if let Some(identity) = identity.as_ref() {
        mutation_request::complete(pool, identity, MutationOperation::Create, Some("created"))
            .await?;
    }
    Ok(id)
}

fn read_reserved_task(
    store: &impl TaskVault,
    project: &Project,
    id: &TaskId,
) -> Result<Option<TaskRecord>, AddTaskError> {
    store
        .get_task(project, id)
        .map_err(|source| AddTaskError::ReadReservedTask {
            id: id.clone(),
            source: anyhow::Error::new(source),
        })
}

fn created_record_matches(
    record: &TaskRecord,
    prepared: &PreparedAdd,
    blocked_by: Option<&pwf_models::task::BlockedBy>,
    command: &AddTask,
) -> bool {
    let stored_blocked_by = match &record.blocked_by {
        StoredBlockedBy::Absent => None,
        StoredBlockedBy::Valid(value) => Some(value),
        StoredBlockedBy::Malformed { .. } => return false,
    };
    let stored_tags = record
        .tags
        .as_ref()
        .map(super::tags::parse_frontmatter)
        .transpose();
    record.status == pwf_models::task::TaskStatus::Active
        && matches!(record.materialization, Materialization::NoteFile)
        && record.title == prepared.title.as_ref()
        && record.body == prepared.body
        && stored_blocked_by == blocked_by
        && record.effort.as_deref() == command.effort.as_ref().map(AsRef::as_ref)
        && record.priority.as_deref() == command.priority.as_ref().map(AsRef::as_ref)
        && stored_tags.is_ok_and(|tags| tags.as_ref() == command.tags.as_ref())
}

fn map_blocked_by_error(error: BlockedByValidationError) -> AddTaskError {
    match error {
        BlockedByValidationError::UnknownIds { ids } => AddTaskError::UnknownBlockedByIds { ids },
        BlockedByValidationError::ReadStore { id, source } => {
            AddTaskError::ReadBlockedBy { id, source }
        }
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

struct PreparedAdd {
    project: Project,
    title: TaskTitle,
    body: String,
}

fn prepare_source(
    command: &AddTask,
    selected_project: &Project,
    lane_configuration: &TaskPromptLanes,
) -> Result<PreparedAdd, AddTaskError> {
    let project = selected_project.clone();
    let (title, body) = match command.prompt.kind() {
        AddTaskPromptKind::Shorthand(prompt) => (
            infer_task_title(prompt, lane_configuration)?,
            render(prompt, lane_configuration),
        ),
        AddTaskPromptKind::Structured { title, lanes } => {
            (title.clone(), render_lanes(lanes, lane_configuration))
        }
    };
    Ok(PreparedAdd {
        project,
        title,
        body,
    })
}

#[cfg(test)]
mod tests;
