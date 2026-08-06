use std::path::PathBuf;

use pwf_models::{
    project::{Project, ProjectSelector},
    task::{EffortTier, Prerequisites, Tags, TaskId, TaskTitle, TaskTitleError},
};
use pwf_wire::project::ProjectStatusFilter;

pub use super::create_task::CreateTaskError;
use super::{
    TaskLanes, TaskSection,
    create_task::{self, CreateTask},
    created_task_output, infer_task_title,
    note_body::{render, render_lanes},
    prerequisites::{self, PrerequisiteValidationError},
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
pub struct AddTaskOk {
    pub id: TaskId,
    pub project: String,
    pub title: String,
    pub note_path: PathBuf,
    pub created_section: Option<String>,
}

/// Carries add diagnostics that remain observable after a store failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddTaskDiagnostics {
    /// Managed project receiving the task.
    pub project: String,
    /// Index section created during insertion, when one was absent.
    pub created_section: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddTaskPrompt {
    Shorthand(String),
    Structured { title: TaskTitle, lanes: TaskLanes },
}

/// Requests creation of one task.
#[derive(Debug, Clone)]
pub struct AddTask {
    /// Managed project name or project ID.
    pub project_selector: Option<ProjectSelector>,
    /// Shorthand or structured task prompt.
    pub prompt: AddTaskPrompt,
    /// Selects the human section.
    pub human: bool,
    /// Prerequisite task IDs.
    pub prerequisites: Option<Prerequisites>,
    /// Optional effort tier.
    pub effort: Option<EffortTier>,
    /// Optional normalized discovery tags.
    pub tags: Option<Tags>,
}

#[derive(Debug, thiserror::Error)]
pub enum AddTaskError {
    #[error(
        "Use shorthand: pwf task add <project> \"<prompt>\"\nOr machine mode: pwf task add <project> --title <title> [lane flags]"
    )]
    Usage,
    #[error(transparent)]
    ProjectResolution(#[from] ResolveProjectError),
    #[error("{0}")]
    QueryProject(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("Unknown --prereq id(s): {}.", format_task_ids(ids))]
    UnknownPrerequisiteIds { ids: Vec<TaskId> },
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
///
/// # Panics
///
/// Panics if the store's `insert` violates its contract by returning a record
/// without a [`pwf_models::task::TaskId`].
#[cqrsy::command]
pub async fn execute(
    cmd: &AddTask,
    store: &(impl TaskStore + IndexEntryStore + IndexSectionStore),
    pool: &sqlx::SqlitePool,
    clock: &impl Clock,
) -> Result<AddTaskOk, AddTaskError> {
    let selector = cmd.project_selector.clone().ok_or(AddTaskError::Usage)?;
    let project = resolve_project::execute(
        ResolveProject {
            selector,
            status: ProjectStatusFilter::ACTIVE,
        },
        pool,
    )
    .await?;

    let mut projects = vec![project.clone()];
    if let Some(prerequisites) = cmd.prerequisites.as_ref() {
        for id in prerequisites::project_ids(prerequisites) {
            if projects.iter().any(|project| project.id == id) {
                continue;
            }
            let project = get_active_project::execute(GetActiveProject { id }, pool)
                .await
                .map_err(|error| AddTaskError::QueryProject(Box::new(error)))?;
            projects.push(project);
        }
    }
    let prereq = cmd
        .prerequisites
        .as_ref()
        .map(|prerequisites| {
            prerequisites::validate_and_merge(None, prerequisites, store, &projects)
                .map_err(map_prerequisite_error)
        })
        .transpose()?;
    let prepared = prepare_source(cmd, &project)?;

    let created = create_task::execute(
        CreateTask {
            project: &prepared.project,
            new: NewTask {
                body: prepared.body,
                title: prepared.title,
                created: clock.today(),
                section: cmd.human.then(|| TaskSection::Human.as_str().to_string()),
                prereq,
                effort: cmd.effort,
                tags: cmd.tags.clone(),
            },
        },
        store,
    )
    .map_err(|source| AddTaskError::WriteStore {
        diagnostics: AddTaskDiagnostics {
            project: prepared.project.title.to_string(),
            created_section: source
                .created_section()
                .map(|(_, section)| section.to_string()),
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
    command
        .project_selector
        .as_ref()
        .ok_or(AddTaskError::Usage)?;
    let project = selected_project.clone();
    let (title, body) = match &command.prompt {
        AddTaskPrompt::Shorthand(prompt) => {
            if prompt.trim().is_empty() {
                return Err(AddTaskError::Usage);
            }
            (infer_task_title(prompt)?, render(prompt))
        }
        AddTaskPrompt::Structured { title, lanes } => (title.clone(), render_lanes(lanes)),
    };
    Ok(PreparedAdd {
        project,
        title,
        body,
    })
}

fn map_prerequisite_error(error: PrerequisiteValidationError) -> AddTaskError {
    match error {
        PrerequisiteValidationError::UnknownIds { ids } => {
            AddTaskError::UnknownPrerequisiteIds { ids }
        }
    }
}

fn format_task_ids(ids: &[TaskId]) -> String {
    ids.iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests;
