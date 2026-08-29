use pwf_models::{
    project::Project,
    task::{TaskId, TaskTimestampError, TaskTitle, TaskTitleError},
};
use pwf_wire::{
    project::{ProjectStatusFilter, ResolveProject},
    task::{AddTask, AddTaskDiagnostics, AddTaskPromptKind, AddedTask},
};

pub use super::task_creation::CreateTaskError;
use super::{
    blocked_by::{self, BlockedByValidationError},
    created_task_output, infer_task_title,
    note_body::{render, render_lanes},
    task_creation::{self, TaskCreation},
};
use crate::{
    ports::{
        clock::Clock,
        task_record::{IndexEntryStore, IndexSectionStore, NewTask, TaskStore},
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
    InvalidTitle(#[from] TaskTitleError),
    #[error("cannot read the task creation time: {0}")]
    Clock(#[from] TaskTimestampError),
    #[error("{source}")]
    WriteStore {
        diagnostics: AddTaskDiagnostics,
        #[source]
        source: CreateTaskError,
    },
}

/// Creates a task record and its open index entry for a mapped project.
#[cqrsy::command]
pub async fn execute(
    cmd: &AddTask,
    store: &(impl TaskStore + IndexEntryStore + IndexSectionStore),
    pool: &sqlx::SqlitePool,
    clock: &impl Clock,
) -> Result<AddedTask, AddTaskError> {
    let project = resolve_project::execute(
        ResolveProject {
            selector: cmd.project_selector.clone(),
            status: ProjectStatusFilter::ActiveOnly,
        },
        pool,
    )
    .await?;
    let id = store
        .next_id(&project)
        .map_err(|source| AddTaskError::AllocateTaskId {
            project: project.title.clone(),
            source: anyhow::Error::new(source),
        })?;

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
    let prepared = prepare_source(cmd, &project)?;

    let created = task_creation::create(
        TaskCreation {
            project: &prepared.project,
            id: &id,
            new: NewTask {
                body: prepared.body,
                title: prepared.title,
                created_at: clock.now()?,
                section: cmd.index_section.task_section(),
                blocked_by,
                effort: cmd.effort,
                tags: cmd.tags.clone(),
            },
        },
        store,
    )
    .map_err(|source| AddTaskError::WriteStore {
        diagnostics: AddTaskDiagnostics {
            project: prepared.project.title.clone(),
            created_section: source.created_section().map(|(_, section)| section.clone()),
        },
        source,
    })?;

    Ok(created_task_output(&prepared.project, created))
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
) -> Result<PreparedAdd, AddTaskError> {
    let project = selected_project.clone();
    let (title, body) = match command.prompt.kind() {
        AddTaskPromptKind::Shorthand(prompt) => (infer_task_title(prompt)?, render(prompt)),
        AddTaskPromptKind::Structured { title, lanes } => (title.clone(), render_lanes(lanes)),
    };
    Ok(PreparedAdd {
        project,
        title,
        body,
    })
}

#[cfg(test)]
mod tests;
