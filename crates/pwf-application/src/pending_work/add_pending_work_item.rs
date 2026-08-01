use std::{path::PathBuf, sync::LazyLock};

use pwf_models::{
    pending_work::{EffortTier, ProjectName, Tags, TaskTitle, TaskTitleError, Timestamp},
    project::Project,
};
use regex::Regex;

use super::{
    logic::pending_work_creation::{added_item, project_mapped},
    prerequisite::PrerequisiteValidationError,
    store_util, tag_policy, title,
};
use crate::{
    ports::{
        clock::Clock,
        pending_work_record::{IndexEntryStore, IndexSectionStore, NewItem, PendingWorkStore},
    },
    project::{
        ProjectStatusFilter,
        get_project::{self, GetProject},
        resolve_project::{self, ResolveProject, ResolveProjectError},
    },
};

/// Reports the persistence phase that failed while creating an item and its index entry.
#[derive(Debug, thiserror::Error)]
pub enum CreateItemError {
    #[error("{0}")]
    ReadSections(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("{0}")]
    InsertRecord(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("{source}")]
    InsertIndex {
        project: ProjectName,
        created_section: Option<String>,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl CreateItemError {
    pub fn created_section(&self) -> Option<(&ProjectName, &str)> {
        match self {
            Self::InsertIndex {
                project,
                created_section: Some(section),
                ..
            } => Some((project, section)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddPendingWorkItemOk {
    pub id: String,
    pub project: String,
    pub title: String,
    pub note_path: PathBuf,
    pub created_section: Option<String>,
}

/// Carries add diagnostics that remain observable after a store failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddPendingWorkDiagnostics {
    /// Managed project receiving the item.
    pub project: String,
    /// Index section created during insertion, when one was absent.
    pub created_section: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AddPendingWorkSource {
    Prompt {
        prompt: String,
        title: Option<TaskTitle>,
    },
    Plan {
        path: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PendingWorkSection {
    Future,
    Human,
    LowPriority,
}

impl PendingWorkSection {
    #[must_use]
    fn from_name(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "future" => Some(Self::Future),
            "human" => Some(Self::Human),
            "low-prio" => Some(Self::LowPriority),
            _ => None,
        }
    }

    #[must_use]
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Future => "Future",
            Self::Human => "Human",
            Self::LowPriority => "Low-prio",
        }
    }
}

/// Requests creation of one pending-work item.
#[derive(Debug, Clone)]
pub struct AddPendingWorkItem {
    /// Managed project name or project ID.
    pub project_identifier: Option<String>,
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
    /// Raw repeated prerequisite values.
    pub prerequisites: Vec<String>,
    /// Optional effort tier.
    pub effort: Option<EffortTier>,
    /// Raw repeated tag values.
    pub tags: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum AddPendingWorkError {
    #[error("Use: pwf add <project> \"<prompt>\"")]
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
    #[error("Invalid --prereq id: {raw}.")]
    InvalidPrerequisiteId { raw: String },
    #[error("--prereq requires an id.")]
    MissingPrerequisiteId,
    #[error("Unknown --prereq id(s): {}.", ids.join(", "))]
    UnknownPrerequisiteIds { ids: Vec<String> },
    #[error(transparent)]
    InvalidTitle(#[from] TaskTitleError),
    #[error("{source}")]
    WriteStore {
        diagnostics: AddPendingWorkDiagnostics,
        #[source]
        source: CreateItemError,
    },
}

/// Creates a pending-work record and its open index entry for a mapped project.
///
/// # Errors
///
/// Returns [`AddPendingWorkError`] when project preparation fails or the store cannot create the
/// item and index entry.
///
/// # Panics
///
/// Panics if the store's `insert` violates its contract by returning a record
/// without a canonical [`pwf_models::pending_work::WorkItemId`].
#[cqrsy::command]
pub async fn execute(
    cmd: &AddPendingWorkItem,
    store: &(impl PendingWorkStore + IndexEntryStore + IndexSectionStore),
    pool: &sqlx::SqlitePool,
    clock: &impl Clock,
) -> Result<AddPendingWorkItemOk, AddPendingWorkError> {
    let identifier = cmd
        .project_identifier
        .as_deref()
        .ok_or(AddPendingWorkError::Usage)?;
    let project = resolve_project::execute(
        ResolveProject {
            identifier: identifier.to_string(),
            status: ProjectStatusFilter::ACTIVE,
        },
        pool,
    )
    .await?;
    let mut projects = vec![project.clone()];
    if !cmd.prerequisites.is_empty() {
        for id in
            super::prerequisite::project_ids(&cmd.prerequisites).map_err(map_prerequisite_error)?
        {
            if projects.iter().any(|project| project.id == id) {
                continue;
            }
            let project = get_project::execute(
                GetProject {
                    id,
                    status: ProjectStatusFilter::ACTIVE,
                },
                pool,
            )
            .await
            .map_err(|error| AddPendingWorkError::QueryProject(Box::new(error)))?;
            projects.push(project);
        }
    }
    execute_with_projects(cmd, store, &project, &projects, clock)
}

fn execute_with_projects(
    cmd: &AddPendingWorkItem,
    store: &(impl PendingWorkStore + IndexEntryStore + IndexSectionStore),
    project: &Project,
    projects: &[Project],
    clock: &impl Clock,
) -> Result<AddPendingWorkItemOk, AddPendingWorkError> {
    let section = resolve_section(cmd.section.as_deref(), cmd.human)?;
    let authored_date = cmd
        .date
        .clone()
        .map_or_else(|| clock.today(), Timestamp::new);
    let tags = parse_tags(&cmd.tags)?;
    let prereq = if cmd.prerequisites.is_empty() {
        None
    } else {
        Some(
            super::prerequisite::validate_and_merge(None, &cmd.prerequisites, store, projects)
                .map_err(map_prerequisite_error)?,
        )
    };
    let prepared = prepare_source(cmd, project)?;

    let created = store_util::create_item(
        store,
        &prepared.project,
        NewItem {
            prompt: prepared.prompt,
            title: prepared.title,
            created: authored_date,
            section: section.map(PendingWorkSection::as_str).map(str::to_string),
            prereq,
            effort: cmd.effort,
            tags,
        },
    )
    .map_err(|source| AddPendingWorkError::WriteStore {
        diagnostics: AddPendingWorkDiagnostics {
            project: prepared.project.title.to_string(),
            created_section: source
                .created_section()
                .map(|(_, section)| section.to_string()),
        },
        source,
    })?;

    Ok(added_item(&prepared.project, created))
}

struct PreparedAdd {
    project: Project,
    title: TaskTitle,
    prompt: String,
}

fn parse_tags(values: &[String]) -> Result<Option<Tags>, AddPendingWorkError> {
    if values.is_empty() {
        return Ok(None);
    }
    tag_policy::parse_values(values)
        .map(Some)
        .map_err(|error| AddPendingWorkError::InvalidTag {
            raw: error.raw().to_string(),
        })
}

fn prepare_source(
    command: &AddPendingWorkItem,
    selected_project: &Project,
) -> Result<PreparedAdd, AddPendingWorkError> {
    command
        .project_identifier
        .as_deref()
        .ok_or(AddPendingWorkError::Usage)?;
    let source = if let Some(path) = command.continue_path.as_ref() {
        Some(AddPendingWorkSource::Plan { path: path.clone() })
    } else if command.prompt.is_empty() {
        None
    } else {
        Some(AddPendingWorkSource::Prompt {
            prompt: command.prompt.clone(),
            title: command.title.clone(),
        })
    };
    let source = source.as_ref().ok_or(AddPendingWorkError::Usage)?;
    if matches!(source, AddPendingWorkSource::Prompt { prompt, .. } if prompt.trim().is_empty()) {
        return Err(AddPendingWorkError::Usage);
    }
    let project = project_mapped(selected_project);
    let (title, prompt) = match source {
        AddPendingWorkSource::Prompt { prompt, title } => (
            title.clone().map_or_else(|| title::inferred(prompt), Ok)?,
            prompt.clone(),
        ),
        AddPendingWorkSource::Plan { path } => (
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
) -> Result<Option<PendingWorkSection>, AddPendingWorkError> {
    match section {
        Some(section) => PendingWorkSection::from_name(section)
            .map(Some)
            .ok_or_else(|| AddPendingWorkError::InvalidSection {
                value: section.to_string(),
            }),
        None => Ok(human.then_some(PendingWorkSection::Human)),
    }
}

fn plan_continuation_prompt(path: &str) -> String {
    format!("continue the plan at {path}")
}

fn plan_title(project: &str, path: &str) -> String {
    static DASH_UNDERSCORE_REGEX: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"[-_]+").expect("valid separator regex"));
    static DATE_SLUG_PREFIX_REGEX: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^\d{4}-\d{2}-\d{2}-").expect("valid dated slug prefix regex")
    });
    let stem = std::path::Path::new(path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("");
    let stem = DATE_SLUG_PREFIX_REGEX.replace(stem, "");
    let excluded = ["kickoff", "plan"];
    let words = DASH_UNDERSCORE_REGEX
        .split(&stem)
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .filter(|word| !excluded.contains(&word.as_str()))
        .collect::<Vec<_>>();
    let project = DASH_UNDERSCORE_REGEX
        .replace_all(project, " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    if words.is_empty() {
        format!("{project} plan")
    } else {
        format!("{project} {}", words.join(" "))
    }
}

fn map_prerequisite_error(error: PrerequisiteValidationError) -> AddPendingWorkError {
    match error {
        PrerequisiteValidationError::InvalidId { raw } => {
            AddPendingWorkError::InvalidPrerequisiteId { raw }
        }
        PrerequisiteValidationError::MissingId => AddPendingWorkError::MissingPrerequisiteId,
        PrerequisiteValidationError::UnknownIds { ids } => {
            AddPendingWorkError::UnknownPrerequisiteIds { ids }
        }
    }
}

#[cfg(test)]
mod tests;
