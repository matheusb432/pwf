use pwf_models::{
    project::{Project, ProjectSelector},
    task::{
        BlockedBy, EffortTier, IndexSection, TaskId, TaskPrompt, TaskTags, TaskTitle,
        TaskTitleError,
    },
};
use pwf_wire::{
    project::ProjectStatusFilter,
    task::{AddTaskDiagnostics, AddedTask},
};

pub use super::task_creation::CreateTaskError;
use super::{
    TaskLanes,
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
        get_active_project::{self, GetActiveProject},
        resolve_project::{self, ResolveProject, ResolveProjectError},
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
enum AddTaskPromptKind {
    Shorthand(TaskPrompt),
    Structured { title: TaskTitle, lanes: TaskLanes },
}

/// Carries one structurally valid shorthand or structured add prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddTaskPrompt(AddTaskPromptKind);

impl AddTaskPrompt {
    /// Creates a non-empty shorthand prompt.
    pub fn shorthand(prompt: TaskPrompt) -> Result<Self, EmptyShorthandPrompt> {
        if prompt.as_ref().trim().is_empty() {
            return Err(EmptyShorthandPrompt);
        }
        Ok(Self(AddTaskPromptKind::Shorthand(prompt)))
    }

    #[must_use]
    pub fn structured(title: TaskTitle, lanes: TaskLanes) -> Self {
        Self(AddTaskPromptKind::Structured { title, lanes })
    }
}

/// Reports a shorthand add prompt without authored content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("task shorthand prompt cannot be empty")]
pub struct EmptyShorthandPrompt;

/// Requests creation of one task.
#[derive(Debug, Clone)]
pub struct AddTask {
    /// Managed project name or project ID.
    pub project_selector: ProjectSelector,
    /// Shorthand or structured task prompt.
    pub prompt: AddTaskPrompt,
    /// Selects the task's index placement.
    pub index_section: IndexSection,
    /// Task IDs in the `blocked_by` relationship.
    pub blocked_by: Option<BlockedBy>,
    /// Optional effort tier.
    pub effort: Option<EffortTier>,
    /// Optional normalized discovery tags.
    pub tags: Option<TaskTags>,
}

#[derive(Debug, thiserror::Error)]
pub enum AddTaskError {
    #[error(transparent)]
    ProjectResolution(#[from] ResolveProjectError),
    #[error("{0}")]
    QueryProject(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("Unknown --blocked-by id(s): {}.", blocked_by::format_task_ids(ids))]
    UnknownBlockedByIds { ids: Vec<TaskId> },
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
            blocked_by::validate_and_merge(None, blocked_by, store, &projects).map_err(
                |BlockedByValidationError::UnknownIds { ids }| AddTaskError::UnknownBlockedByIds {
                    ids,
                },
            )
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
    let (title, body) = match &command.prompt.0 {
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
