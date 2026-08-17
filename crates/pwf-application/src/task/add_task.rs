use pwf_models::{
    project::Project,
    task::{TaskId, TaskTitle, TaskTitleError},
};
use pwf_wire::{
    project::{GetActiveProject, ProjectStatusFilter, ResolveProject},
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
        get_active_project,
        resolve_project::{self, ResolveProjectError},
    },
};

#[derive(Debug, thiserror::Error)]
pub enum AddTaskError {
    #[error(transparent)]
    ProjectResolution(#[from] ResolveProjectError),
    #[error("{0}")]
    QueryProject(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("Unknown --blocked-by id(s): {}.", blocked_by::format_task_ids(ids))]
    UnknownBlockedByIds { ids: Vec<TaskId> },
    #[error("cannot validate --blocked-by task {id}: {source}")]
    ReadBlockedBy {
        id: TaskId,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error(transparent)]
    InvalidTitle(#[from] TaskTitleError),
    #[error("{source}")]
    WriteStore {
        diagnostics: AddTaskDiagnostics,
        #[source]
        source: CreateTaskError,
    },
}

/// Creates a task record and its open index entry for a mapped project.
///
/// # Errors
///
/// Returns [`AddTaskError`] when project preparation fails or the store cannot create the
/// task and index entry.
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

    let mut projects = vec![project.clone()];
    if let Some(blocked_by) = cmd.blocked_by.as_ref() {
        for id in blocked_by::project_ids(blocked_by) {
            if projects.iter().any(|project| project.id == id) {
                continue;
            }
            let project = get_active_project::execute(GetActiveProject { id }, pool)
                .await
                .map_err(|error| AddTaskError::QueryProject(Box::new(error)))?;
            projects.push(project);
        }
    }
    let blocked_by = cmd
        .blocked_by
        .as_ref()
        .map(|blocked_by| {
            blocked_by::validate_and_merge(None, blocked_by, store, &projects)
                .map_err(map_blocked_by_error)
        })
        .transpose()?;
    let prepared = prepare_source(cmd, &project)?;

    let created = task_creation::create(
        TaskCreation {
            project: &prepared.project,
            new: NewTask {
                body: prepared.body,
                title: prepared.title,
                created: clock.today(),
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
