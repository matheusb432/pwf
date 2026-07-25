use std::{path::PathBuf, sync::LazyLock};

use pwf_domain::pending_work::{
    HANDOFF_TAG, ProjectName, Tags, Timestamp, WorkItemId, inferred_title, normalize_title,
    title_was_normalized,
};
use regex::Regex;

use super::{
    prerequisite::PrerequisiteValidationError,
    project_registry::{ProjectRegistry, ProjectResolutionError},
    store_util,
};
use crate::{
    HandoffDocumentStore, HandoffLedger,
    handoff::{HandoffError, HandoffMutationOk, lifecycle},
    ports::{AppRecordStore, IndexEntry, IndexSection, NewItem, PendingWorkItem},
};

/// Describes the pending-work item and index section created by [`execute`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddedItem {
    /// Canonical identifier allocated to the item.
    pub id: String,
    /// Managed project containing the item.
    pub project: String,
    /// Persisted item title.
    pub title: String,
    /// Path of the created item note.
    pub note_path: PathBuf,
    /// Index section created during insertion, when one was absent.
    pub created_section: Option<String>,
    /// Whether an explicit title required metadata-safe normalization.
    pub title_normalized: bool,
    /// Linked handoff side effect.
    pub handoff: HandoffMutationOk,
}

/// Carries add diagnostics that remain observable after a later handoff failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddPendingWorkDiagnostics {
    /// Managed project receiving the item.
    pub project: String,
    /// Index section created during insertion, when one was absent.
    pub created_section: Option<String>,
    /// Whether an explicit title required metadata-safe normalization.
    pub title_normalized: bool,
}

/// Selects the semantic source used to create a pending-work item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddPendingWorkSource {
    /// Creates an item from a direct prompt and optional explicit title.
    Prompt {
        /// Pending-work prompt.
        prompt: String,
        /// Optional explicit title.
        title: Option<String>,
    },
    /// Creates an item that continues a plan path.
    Plan {
        /// Plan path retained in the generated prompt.
        path: String,
    },
    /// Creates an item from the newest handoff in the managed repository.
    NewestHandoff,
}

/// Selects one canonical pending-work index section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingWorkSection {
    /// Work intentionally deferred to a future queue.
    Future,
    /// Work requiring human action.
    Human,
    /// Work kept in the low-priority queue.
    LowPriority,
}

impl PendingWorkSection {
    /// Parses a canonical section name case-insensitively.
    #[must_use]
    pub fn from_name(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "future" => Some(Self::Future),
            "human" => Some(Self::Human),
            "low-prio" => Some(Self::LowPriority),
            _ => None,
        }
    }

    /// Returns the canonical persisted section label.
    #[must_use]
    pub fn as_str(self) -> &'static str {
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
    /// Managed project name or identifier prefix.
    pub project_identifier: Option<String>,
    /// Semantic prompt source.
    pub source: Option<AddPendingWorkSource>,
    /// Authored creation date.
    pub created: String,
    /// Optional canonical index section.
    pub section: Option<PendingWorkSection>,
    /// Raw repeated prerequisite values.
    pub prerequisites: Vec<String>,
    /// Optional effort tier.
    pub effort: Option<u8>,
    /// Raw repeated tag values.
    pub tags: Vec<String>,
}

/// Reports add preparation or persistence failures.
#[derive(Debug, thiserror::Error)]
pub enum AddPendingWorkError {
    /// Required positional add input was absent.
    #[error("Use: pwf add <project> \"<prompt>\"")]
    Usage,
    /// The managed project identifier could not be resolved uniquely.
    #[error(transparent)]
    ProjectResolution(#[from] ProjectResolutionError),
    /// The selected project has no usable directory source.
    #[error("Project '{project}' has no directory source; update the managed project record.")]
    ProjectHasNoDirectorySource {
        /// Requested project name.
        project: String,
    },
    /// A raw tag was invalid.
    #[error(
        "Invalid --tag value {raw:?}; use lowercase/uppercase ASCII letters, digits, '_' or '-', without leading, trailing, or repeated separators."
    )]
    InvalidTag {
        /// Rejected raw value.
        raw: String,
    },
    /// A raw prerequisite identifier was invalid.
    #[error("Invalid --prereq id: {raw}.")]
    InvalidPrerequisiteId {
        /// Rejected raw value.
        raw: String,
    },
    /// A prerequisite flag contained no identifier.
    #[error("--prereq requires an id.")]
    MissingPrerequisiteId,
    /// One or more prerequisite records were absent.
    #[error("Unknown --prereq id(s): {}.", ids.join(", "))]
    UnknownPrerequisiteIds {
        /// Canonical missing identifiers.
        ids: Vec<String>,
    },
    /// A linked handoff failed read-only validation.
    #[error(transparent)]
    HandoffPreflight(HandoffError),
    /// Pending work was created before its linked handoff failed.
    #[error("{pending_work_identifier} was mutated, but its handoff was not: {source}")]
    HandoffAfterPendingWork {
        /// Created pending-work identifier.
        pending_work_identifier: WorkItemId,
        /// Diagnostics produced by the successful pending-work insertion.
        diagnostics: Box<AddPendingWorkDiagnostics>,
        /// Handoff failure after pending-work persistence.
        #[source]
        source: HandoffError,
    },
    /// An item or index write failed.
    #[error("{source}")]
    WriteStore {
        diagnostics: AddPendingWorkDiagnostics,
        #[source]
        source: store_util::CreateItemError,
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
/// without a canonical [`pwf_domain::pending_work::WorkItemId`].
#[cqrsy::command]
pub fn execute<S>(
    cmd: AddPendingWorkItem,
    store: &S,
    projects: &ProjectRegistry,
) -> Result<AddedItem, AddPendingWorkError>
where
    S: AppRecordStore<PendingWorkItem>
        + AppRecordStore<IndexEntry>
        + AppRecordStore<IndexSection>
        + HandoffDocumentStore
        + AppRecordStore<HandoffLedger>,
{
    let tags = parse_tags(&cmd.tags)?;
    let prereq = if cmd.prerequisites.is_empty() {
        None
    } else {
        Some(
            super::prerequisite::validate_and_merge(None, &cmd.prerequisites, store, projects)
                .map_err(map_prerequisite_error)?,
        )
    };
    let prepared = prepare_source(&cmd, store, projects)?;
    let scaffold = if !prepared.newest_handoff
        && tags
            .as_ref()
            .is_some_and(|tags| tags.contains_name(HANDOFF_TAG))
    {
        Some(
            lifecycle::preflight_scaffold(
                store,
                &prepared.repository,
                &prepared.project,
                &prepared.title,
                &cmd.created,
            )
            .map_err(AddPendingWorkError::HandoffPreflight)?,
        )
    } else {
        None
    };

    let created = store_util::create_item(
        store,
        &prepared.project,
        NewItem {
            prompt: prepared.prompt,
            title: Some(prepared.title),
            created: Timestamp::new(cmd.created),
            section: cmd
                .section
                .map(PendingWorkSection::as_str)
                .map(str::to_string),
            prereq,
            effort: cmd.effort,
            tags,
        },
    )
    .map_err(|source| AddPendingWorkError::WriteStore {
        diagnostics: AddPendingWorkDiagnostics {
            project: prepared.project.to_string(),
            created_section: source
                .created_section()
                .map(|(_, section)| section.to_string()),
            title_normalized: prepared.title_normalized,
        },
        source,
    })?;

    let pending_work_identifier = created
        .record
        .id
        .as_item()
        .expect("inserted record carries a canonical id")
        .clone();
    let diagnostics = AddPendingWorkDiagnostics {
        project: prepared.project.to_string(),
        created_section: created.created_section.clone(),
        title_normalized: prepared.title_normalized,
    };
    let handoff = scaffold
        .map(|scaffold| lifecycle::commit_scaffold(store, scaffold, &pending_work_identifier))
        .transpose()
        .map_err(|source| AddPendingWorkError::HandoffAfterPendingWork {
            pending_work_identifier,
            diagnostics: Box::new(diagnostics.clone()),
            source,
        })?
        .unwrap_or(HandoffMutationOk::NotLinked);

    Ok(added_item(
        &prepared.project,
        created,
        prepared.title_normalized,
        handoff,
    ))
}

struct PreparedAdd {
    project: ProjectName,
    repository: String,
    title: String,
    prompt: String,
    title_normalized: bool,
    newest_handoff: bool,
}

fn parse_tags(values: &[String]) -> Result<Option<Tags>, AddPendingWorkError> {
    if values.is_empty() {
        return Ok(None);
    }
    Tags::parse_values(values)
        .map(Some)
        .map_err(|error| AddPendingWorkError::InvalidTag {
            raw: error.raw().to_string(),
        })
}

fn prepare_source<S>(
    command: &AddPendingWorkItem,
    store: &S,
    projects: &ProjectRegistry,
) -> Result<PreparedAdd, AddPendingWorkError>
where
    S: HandoffDocumentStore,
{
    let identifier = command
        .project_identifier
        .as_deref()
        .ok_or(AddPendingWorkError::Usage)?;
    let source = command.source.as_ref().ok_or(AddPendingWorkError::Usage)?;
    if matches!(source, AddPendingWorkSource::Prompt { prompt, .. } if prompt.trim().is_empty()) {
        return Err(AddPendingWorkError::Usage);
    }
    let project = project_mapped(identifier, projects)?;
    let repository = projects
        .repo_for(&project)
        .expect("project_mapped requires a repository")
        .to_string();
    let (title, prompt, title_normalized, newest_handoff) = match source {
        AddPendingWorkSource::Prompt { prompt, title } => {
            let (title, normalized) = match title.as_deref() {
                Some(title) if !title.trim().is_empty() => {
                    (normalize_title(title), title_was_normalized(title))
                }
                _ => (inferred_title(prompt), false),
            };
            (title, prompt.clone(), normalized, false)
        }
        AddPendingWorkSource::Plan { path } => (
            plan_title(project.as_ref(), path),
            plan_continuation_prompt(path),
            false,
            false,
        ),
        AddPendingWorkSource::NewestHandoff => {
            let (title, prompt) = lifecycle::newest_handoff(store, &repository)
                .map_err(AddPendingWorkError::HandoffPreflight)?;
            (title, prompt, false, true)
        }
    };
    Ok(PreparedAdd {
        project,
        repository,
        title,
        prompt,
        title_normalized,
        newest_handoff,
    })
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
    let excluded = ["kickoff", "handoff", "plan"];
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

pub(super) fn project_mapped(
    project_identifier: &str,
    projects: &ProjectRegistry,
) -> Result<ProjectName, AddPendingWorkError> {
    let project = projects.resolve(project_identifier)?.clone();
    let not_mapped = || AddPendingWorkError::ProjectHasNoDirectorySource {
        project: project.to_string(),
    };
    if projects
        .repo_for(&project)
        .is_none_or(|repo| repo.trim().is_empty())
    {
        return Err(not_mapped());
    }
    Ok(project)
}

pub(super) fn added_item(
    project: &ProjectName,
    created: store_util::CreatedItem,
    title_normalized: bool,
    handoff: HandoffMutationOk,
) -> AddedItem {
    let id = created
        .record
        .id
        .as_item()
        .expect("inserted record carries a canonical id")
        .as_ref()
        .to_string();
    AddedItem {
        id,
        project: project.as_ref().to_string(),
        title: created.record.title,
        note_path: PathBuf::from(created.record.locator),
        created_section: created.created_section,
        title_normalized,
        handoff,
    }
}

#[cfg(test)]
mod tests;
