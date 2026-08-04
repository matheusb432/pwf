use std::path::PathBuf;

use pwf_models::{
    project::{Project, ProjectSelector},
    task::{EffortTier, Prerequisites, Tags, TaskId, TaskTitle, TaskTitleError, Timestamp},
};
use pwf_wire::project::ProjectStatusFilter;

pub use super::create_task::CreateTaskError;
use super::{
    TaskSection,
    create_task::{self, CreateTask},
    created_task_output, infer_task_title,
    prerequisites::{self, PrerequisiteValidationError},
    tags,
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
enum AddTaskSource {
    Prompt {
        prompt: String,
        title: Option<TaskTitle>,
    },
    Plan {
        path: String,
    },
}

/// Requests creation of one task.
#[derive(Debug, Clone)]
pub struct AddTask {
    /// Managed project name or project ID.
    pub project_selector: Option<ProjectSelector>,
    /// Direct prompt text after transport-level word joining.
    pub prompt: String,
    /// Optional plan path selected by `--continue`.
    pub continue_path: Option<String>,
    /// Optional explicit title for a direct prompt.
    pub title: Option<TaskTitle>,
    /// Optional authored creation date.
    pub date: Option<String>,
    /// Optional raw section selected by `--section`.
    pub section: Option<String>,
    /// Selects the human section when no explicit section is supplied.
    pub human: bool,
    /// Prerequisite task IDs.
    pub prerequisites: Option<Prerequisites>,
    /// Optional effort tier.
    pub effort: Option<EffortTier>,
    /// Raw repeated tag values.
    pub tags: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum AddTaskError {
    #[error("Use: pwf task add <project> \"<prompt>\"")]
    Usage,
    #[error("Unknown --section value '{value}'. Use one of: future, human, low-prio.")]
    InvalidSection { value: String },
    #[error(transparent)]
    ProjectResolution(#[from] ResolveProjectError),
    #[error("{0}")]
    QueryProject(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error(
        "Invalid --tag value {raw:?}; use lowercase/uppercase ASCII letters, digits, '_' or '-', without leading, trailing, or repeated separators."
    )]
    InvalidTag { raw: String },
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
    let section = resolve_section(cmd.section.as_deref(), cmd.human)?;
    let authored_date = cmd
        .date
        .clone()
        .map_or_else(|| clock.today(), Timestamp::new);
    let tags = parse_tags(&cmd.tags)?;
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
                prompt: prepared.prompt,
                title: prepared.title,
                created: authored_date,
                section: section.map(TaskSection::as_str).map(str::to_string),
                prereq,
                effort: cmd.effort,
                tags,
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
    prompt: String,
}

fn parse_tags(values: &[String]) -> Result<Option<Tags>, AddTaskError> {
    if values.is_empty() {
        return Ok(None);
    }
    tags::parse_values(values)
        .map(Some)
        .map_err(|error| AddTaskError::InvalidTag {
            raw: error.raw().to_string(),
        })
}

fn prepare_source(
    command: &AddTask,
    selected_project: &Project,
) -> Result<PreparedAdd, AddTaskError> {
    command
        .project_selector
        .as_ref()
        .ok_or(AddTaskError::Usage)?;
    let source = if let Some(path) = command.continue_path.as_ref() {
        Some(AddTaskSource::Plan { path: path.clone() })
    } else if command.prompt.is_empty() {
        None
    } else {
        Some(AddTaskSource::Prompt {
            prompt: command.prompt.clone(),
            title: command.title.clone(),
        })
    };
    let source = source.as_ref().ok_or(AddTaskError::Usage)?;
    if matches!(source, AddTaskSource::Prompt { prompt, .. } if prompt.trim().is_empty()) {
        return Err(AddTaskError::Usage);
    }
    let project = selected_project.clone();
    let (title, prompt) = match source {
        AddTaskSource::Prompt { prompt, title } => (
            title.clone().map_or_else(|| infer_task_title(prompt), Ok)?,
            prompt.clone(),
        ),
        AddTaskSource::Plan { path } => (
            TaskTitle::try_new(plan_title(project.title.as_ref(), path))?,
            plan_continuation_prompt(path),
        ),
    };
    Ok(PreparedAdd {
        project,
        title,
        prompt,
    })
}

fn resolve_section(
    section: Option<&str>,
    human: bool,
) -> Result<Option<TaskSection>, AddTaskError> {
    match section {
        Some(section) => {
            TaskSection::from_name(section)
                .map(Some)
                .ok_or_else(|| AddTaskError::InvalidSection {
                    value: section.to_string(),
                })
        }
        None => Ok(human.then_some(TaskSection::Human)),
    }
}

fn plan_continuation_prompt(path: &str) -> String {
    format!("continue the plan at {path}")
}

fn plan_title(project: &str, path: &str) -> String {
    let stem = std::path::Path::new(path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("");
    let stem = strip_date_slug_prefix(stem);
    let excluded = ["kickoff", "plan"];
    let words = stem
        .split(['-', '_'])
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .filter(|word| !excluded.contains(&word.as_str()))
        .collect::<Vec<_>>();
    let project = project
        .split(['-', '_'])
        .flat_map(str::split_whitespace)
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    if words.is_empty() {
        format!("{project} plan")
    } else {
        format!("{project} {}", words.join(" "))
    }
}

fn strip_date_slug_prefix(stem: &str) -> &str {
    let Some(prefix) = stem.get(..11) else {
        return stem;
    };
    let bytes = prefix.as_bytes();
    let is_date_prefix = bytes[0..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(u8::is_ascii_digit)
        && bytes[10] == b'-';
    if is_date_prefix { &stem[11..] } else { stem }
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
