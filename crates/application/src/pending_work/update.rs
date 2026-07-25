use pwf_domain::pending_work::{ProjectName, Tags, WorkItemId, WorkItemStatus};

use super::{
    commit_provenance, identifier,
    note_body::{append_lanes, append_report_block, render},
    prerequisite::{PrerequisiteValidationError, validate_and_merge},
    project_registry::ProjectRegistry,
    store_util::body_region,
    tag_policy, title,
};
use crate::ports::{AppRecordStore, ItemPatch, PendingWorkItem};

/// Requests edits to one pending-work item.
#[derive(Debug, Clone)]
pub struct UpdatePendingWorkItem {
    /// Requested work-item identifier.
    pub id: String,
    /// Optional replacement prompt.
    pub prompt: Option<String>,
    /// Optional replacement title.
    pub title: Option<String>,
    /// Optional lane content appended to the body.
    pub append: Option<String>,
    /// Raw prerequisite values appended to existing prerequisites.
    pub prereq: Vec<String>,
    /// Whether existing prerequisites are cleared.
    pub clear_prereq: bool,
    /// Raw repeated commit ranges.
    pub commits: Vec<String>,
    /// Optional report appended to the body.
    pub append_report: Option<String>,
    /// Optional replacement effort tier.
    pub effort: Option<u8>,
    /// Raw tags appended to existing tags.
    pub tags: Vec<String>,
    /// Whether existing tags are cleared before applying raw tags.
    pub tags_clear: bool,
}

/// Reports update preparation or persistence failures.
#[derive(Debug, thiserror::Error)]
pub enum UpdatePendingWorkError {
    /// The requested item does not exist.
    #[error("Open pending-work item not found: {id}")]
    ItemNotFound {
        /// Requested identifier.
        id: String,
    },
    /// The request contains no effective edit.
    #[error(
        "nothing to update (pass --prompt, --title, --prereq, --clear-prereq, --tag, --tags-clear, --commits, --append-report, --append, and/or --effort)."
    )]
    NothingToUpdate,
    /// A closed item was asked to accept an open-item edit.
    #[error(
        "only --commits / --append-report can amend closed item {id} (done/cancelled); body/title/prereq/tags/append/effort need an open item."
    )]
    ClosedItemAmendOnly {
        /// Requested closed-item identifier.
        id: String,
    },
    /// Existing tag frontmatter cannot be merged safely.
    #[error("item {id} has invalid tags frontmatter: {raw:?}.")]
    InvalidTagsFrontmatter {
        /// Item containing invalid frontmatter.
        id: String,
        /// Raw frontmatter value.
        raw: String,
    },
    /// A raw tag value is invalid.
    #[error(
        "Invalid --tag value {raw:?}; use lowercase/uppercase ASCII letters, digits, '_' or '-', without leading, trailing, or repeated separators."
    )]
    InvalidTag {
        /// Raw tag value.
        raw: String,
    },
    /// A prerequisite value is not a work-item identifier.
    #[error("Invalid --prereq id: {raw}.")]
    InvalidPrereqId {
        /// Raw prerequisite value.
        raw: String,
    },
    /// No prerequisite identifier was supplied.
    #[error("--prereq requires an id.")]
    MissingPrereqId,
    /// One or more prerequisite records do not exist.
    #[error("Unknown --prereq id(s): {}.", ids.join(", "))]
    UnknownPrereqIds {
        /// Canonical identifiers without records.
        ids: Vec<String>,
    },
    /// The requested report is blank.
    #[error("--report cannot be empty.")]
    EmptyReport,
    /// The requested lane append is blank.
    #[error("--append cannot be empty.")]
    EmptyAppend,
    /// An item or prerequisite store operation failed.
    #[error("{0}")]
    WriteStore(#[source] Box<dyn std::error::Error + Send + Sync>),
}

/// Describes either an open-item edit or a closed-item amendment from [`execute`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdatePendingWorkItemOk {
    /// Reports the effective identity and title after editing an open item.
    OpenItemEdit {
        /// Canonical identifier of the edited item.
        id: String,
        /// Managed project containing the item.
        project: String,
        /// Effective title after the edit.
        title: String,
        /// Whether YAML-safe normalization changed an explicit title.
        title_normalized: bool,
    },
    /// Reports the fields amended on a closed item.
    Changed {
        /// Canonical identifier of the amended item.
        id: String,
        /// Renderable summaries of the applied changes.
        changes: Vec<String>,
    },
}

impl UpdatePendingWorkItemOk {
    /// Reports whether YAML-safe normalization changed an explicit title.
    #[must_use]
    pub fn title_normalized(&self) -> bool {
        match self {
            Self::OpenItemEdit {
                title_normalized, ..
            } => *title_normalized,
            Self::Changed { .. } => false,
        }
    }
}

struct PendingWorkItemIdentity {
    project: ProjectName,
    identifier: WorkItemId,
}

pub(crate) struct PreparedPendingWorkUpdate {
    identity: PendingWorkItemIdentity,
    patch: ItemPatch,
    outcome: UpdatePendingWorkItemOk,
}

/// Applies body and frontmatter edits in one item patch.
///
/// Closed items accept only commit and report amendments.
///
/// # Errors
///
/// Returns [`UpdatePendingWorkError`] when preparation, validation, or persistence fails.
#[cqrsy::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "preserves the public request-first operation signature"
)]
pub fn execute<S>(
    command: UpdatePendingWorkItem,
    store: &S,
    projects: &ProjectRegistry,
) -> Result<UpdatePendingWorkItemOk, UpdatePendingWorkError>
where
    S: AppRecordStore<PendingWorkItem>,
{
    let prepared = prepare(&command, store, projects)?;
    persist(prepared, store)
}

pub(crate) fn prepare(
    command: &UpdatePendingWorkItem,
    store: &impl AppRecordStore<PendingWorkItem>,
    projects: &ProjectRegistry,
) -> Result<PreparedPendingWorkUpdate, UpdatePendingWorkError> {
    let commits = commit_provenance::normalize(&command.commits);
    let tags = parse_tags(&command.tags)?;
    let edits_body = command.prompt.is_some()
        || command.title.is_some()
        || !command.prereq.is_empty()
        || command.clear_prereq
        || command.append.is_some()
        || command.effort.is_some()
        || tags.is_some()
        || command.tags_clear;
    if !edits_body && commits.is_none() && command.append_report.is_none() {
        return Err(UpdatePendingWorkError::NothingToUpdate);
    }

    let not_found = || UpdatePendingWorkError::ItemNotFound {
        id: command.id.clone(),
    };
    let identifier = identifier::parse(&command.id).ok_or_else(not_found)?;
    let project = projects.project_for_id(&identifier).ok_or_else(not_found)?;
    let record = store
        .get(project, &identifier)
        .map_err(|error| UpdatePendingWorkError::WriteStore(Box::new(error)))?
        .ok_or_else(not_found)?;
    let identity = PendingWorkItemIdentity {
        project: project.clone(),
        identifier,
    };

    if record.status == WorkItemStatus::Active {
        prepare_open_item(
            store,
            projects,
            identity,
            command,
            commits.as_deref(),
            tags.as_ref(),
            &record,
        )
    } else if edits_body {
        Err(UpdatePendingWorkError::ClosedItemAmendOnly {
            id: command.id.clone(),
        })
    } else {
        amend_closed_item(identity, command, commits.as_deref(), &record)
    }
}

pub(crate) fn persist(
    prepared: PreparedPendingWorkUpdate,
    store: &impl AppRecordStore<PendingWorkItem>,
) -> Result<UpdatePendingWorkItemOk, UpdatePendingWorkError> {
    store
        .update(
            &prepared.identity.project,
            &prepared.identity.identifier,
            prepared.patch,
        )
        .map_err(|error| UpdatePendingWorkError::WriteStore(Box::new(error)))?;
    Ok(prepared.outcome)
}

fn prepare_open_item<S>(
    store: &S,
    projects: &ProjectRegistry,
    identity: PendingWorkItemIdentity,
    command: &UpdatePendingWorkItem,
    commits: Option<&str>,
    tags: Option<&Tags>,
    record: &PendingWorkItem,
) -> Result<PreparedPendingWorkUpdate, UpdatePendingWorkError>
where
    S: AppRecordStore<PendingWorkItem>,
{
    let new_title = command
        .title
        .as_deref()
        .map_or_else(|| record.title.clone(), title::normalize);

    let mut patch = ItemPatch {
        body: compute_body(command, record)?,
        ..ItemPatch::default()
    };
    if command.title.is_some() {
        patch.title = Some(new_title.clone());
    }
    if command.clear_prereq {
        patch.prereq = Some(None);
    } else if !command.prereq.is_empty() {
        let merged = validate_and_merge(record.prereq.as_deref(), &command.prereq, store, projects)
            .map_err(map_prerequisite_error)?;
        patch.prereq = Some(Some(merged));
    }
    if let Some(commits) = commits {
        patch.commits = Some(Some(commits.to_string()));
    }
    if let Some(effort) = command.effort {
        patch.effort = Some(effort);
    }
    patch.tags = resolve_tags(tags, command.tags_clear, &identity.identifier, record)?;

    let outcome = UpdatePendingWorkItemOk::OpenItemEdit {
        id: identity.identifier.as_ref().to_string(),
        project: identity.project.as_ref().to_string(),
        title: new_title,
        title_normalized: command.title.as_deref().is_some_and(title::was_normalized),
    };
    Ok(PreparedPendingWorkUpdate {
        identity,
        patch,
        outcome,
    })
}

/// Computes a replacement body when a prompt, lane, or report edit requires one.
///
/// Prompt replacement starts from a new body; lane and report edits build on that result.
fn compute_body(
    command: &UpdatePendingWorkItem,
    record: &PendingWorkItem,
) -> Result<Option<String>, UpdatePendingWorkError> {
    let base = body_region(&record.body);
    let mut body: Option<String> = command.prompt.as_deref().map(render);
    if let Some(append) = &command.append {
        let current = body.as_deref().unwrap_or(base);
        body = Some(append_lanes(current, append).ok_or(UpdatePendingWorkError::EmptyAppend)?);
    }
    if let Some(report) = &command.append_report {
        let current = body.as_deref().unwrap_or(base);
        body =
            Some(append_report_block(current, report).ok_or(UpdatePendingWorkError::EmptyReport)?);
    }
    Ok(body)
}

/// Resolves the tri-state `ItemPatch.tags` value: unchanged, cleared, or replaced.
#[expect(
    clippy::option_option,
    reason = "preserves ItemPatch.tags tri-state semantics"
)]
fn resolve_tags(
    appended: Option<&Tags>,
    clear: bool,
    id: &WorkItemId,
    record: &PendingWorkItem,
) -> Result<Option<Option<Tags>>, UpdatePendingWorkError> {
    let Some(appended) = appended else {
        return Ok(clear.then_some(None));
    };
    let tags = if clear {
        appended.clone()
    } else if let Some(existing) = record.tags.as_deref() {
        let existing = tag_policy::parse_frontmatter(existing).map_err(|error| {
            UpdatePendingWorkError::InvalidTagsFrontmatter {
                id: id.as_ref().to_string(),
                raw: error.raw().to_string(),
            }
        })?;
        tag_policy::merge(&existing, appended)
    } else {
        appended.clone()
    };
    Ok(Some(Some(tags)))
}

fn amend_closed_item(
    identity: PendingWorkItemIdentity,
    command: &UpdatePendingWorkItem,
    commits: Option<&str>,
    record: &PendingWorkItem,
) -> Result<PreparedPendingWorkUpdate, UpdatePendingWorkError> {
    let mut patch = ItemPatch::default();
    let mut changes = Vec::new();
    if let Some(commits) = commits {
        patch.commits = Some(Some(commits.to_string()));
        changes.push(commits_change(commits));
    }
    if let Some(report) = &command.append_report {
        let base = body_region(&record.body);
        patch.body =
            Some(append_report_block(base, report).ok_or(UpdatePendingWorkError::EmptyReport)?);
        changes.push("report appended".to_string());
    }
    let outcome = UpdatePendingWorkItemOk::Changed {
        id: identity.identifier.as_ref().to_string(),
        changes,
    };
    Ok(PreparedPendingWorkUpdate {
        identity,
        patch,
        outcome,
    })
}

fn commits_change(commits: &str) -> String {
    format!("commits: {commits}")
}

fn map_prerequisite_error(error: PrerequisiteValidationError) -> UpdatePendingWorkError {
    match error {
        PrerequisiteValidationError::InvalidId { raw } => {
            UpdatePendingWorkError::InvalidPrereqId { raw }
        }
        PrerequisiteValidationError::MissingId => UpdatePendingWorkError::MissingPrereqId,
        PrerequisiteValidationError::UnknownIds { ids } => {
            UpdatePendingWorkError::UnknownPrereqIds { ids }
        }
    }
}

fn parse_tags(values: &[String]) -> Result<Option<Tags>, UpdatePendingWorkError> {
    if values.is_empty() {
        return Ok(None);
    }
    tag_policy::parse_values(values)
        .map(Some)
        .map_err(|error| UpdatePendingWorkError::InvalidTag {
            raw: error.raw().to_string(),
        })
}

#[cfg(test)]
mod tests {
    use pwf_domain::pending_work::{ProjectName, Timestamp, WorkItemId, WorkItemStatus};

    use super::{
        ProjectRegistry, UpdatePendingWorkError, UpdatePendingWorkItem, UpdatePendingWorkItemOk,
    };
    use crate::{Materialization, PendingWorkItem, RecordId, testing::InMemoryStore};

    const S: &str = "\n\n";

    fn registry() -> ProjectRegistry {
        ProjectRegistry::new(vec![(
            ProjectName::try_new("glep-shimeji").unwrap(),
            Some("/repo".to_string()),
            Some("GLP".to_string()),
        )])
    }

    fn record(id: &str, status: WorkItemStatus, body: &str) -> PendingWorkItem {
        PendingWorkItem {
            id: RecordId::Item(WorkItemId::try_new(id).unwrap()),
            title: "tray gui".to_string(),
            status,
            created: Some(Timestamp::new("2026-01-01")),
            completed: (status != WorkItemStatus::Active).then(|| Timestamp::new("2026-06-20")),
            commits: None,
            tags: None,
            effort: None,
            prereq: None,
            section: None,
            body: body.to_string(),
            source: format!("---\nstatus: {status}\n---\n{body}"),
            locator: format!("/mem/glep-shimeji/{id}.md"),
            placement: None,
            materialization: Materialization::NoteFile,
        }
    }

    fn staged(status: WorkItemStatus, body: &str) -> InMemoryStore {
        InMemoryStore::default()
            .with_prefix("glep-shimeji", "GLP")
            .with_project("glep-shimeji", vec![record("GLP-0001", status, body)])
    }

    fn empty(id: &str) -> UpdatePendingWorkItem {
        UpdatePendingWorkItem {
            id: id.to_string(),
            prompt: None,
            title: None,
            append: None,
            prereq: Vec::new(),
            clear_prereq: false,
            commits: Vec::new(),
            append_report: None,
            effort: None,
            tags: Vec::new(),
            tags_clear: false,
        }
    }

    #[test]
    fn update_rejects_empty_patch_with_nothing_to_update() {
        let store = staged(WorkItemStatus::Active, "## Goals\n- x\n");

        let error = super::execute(empty("GLP-0001"), &store, &registry()).unwrap_err();

        assert!(matches!(error, UpdatePendingWorkError::NothingToUpdate));
        assert_eq!(
            error.to_string(),
            "nothing to update (pass --prompt, --title, --prereq, --clear-prereq, --tag, --tags-clear, --commits, --append-report, --append, and/or --effort)."
        );
    }

    #[test]
    fn update_allows_commits_amend_on_closed_item() {
        let store = staged(WorkItemStatus::Done, "## Goals\n- x\n");
        let cmd = UpdatePendingWorkItem {
            commits: vec![" abc..def, ghi..jkl ".to_string(), "abc..def".to_string()],
            ..empty("GLP-0001")
        };

        let updated = super::execute(cmd, &store, &registry()).unwrap();

        assert_eq!(
            updated,
            UpdatePendingWorkItemOk::Changed {
                id: "GLP-0001".to_string(),
                changes: vec!["commits: abc..def, ghi..jkl".to_string()],
            }
        );
        assert_eq!(
            store.items("glep-shimeji")[0].commits.as_deref(),
            Some("abc..def, ghi..jkl")
        );
    }

    #[test]
    fn update_rejects_body_edit_on_closed_item() {
        let store = staged(WorkItemStatus::Done, "body\n");
        let cmd = UpdatePendingWorkItem {
            title: Some("new title".to_string()),
            ..empty("GLP-0001")
        };

        let error = super::execute(cmd, &store, &registry()).unwrap_err();

        assert!(matches!(
            error,
            UpdatePendingWorkError::ClosedItemAmendOnly { ref id } if id == "GLP-0001"
        ));
    }

    #[test]
    fn update_open_item_edits_title_and_body_and_reports_open_edit() {
        let store = staged(WorkItemStatus::Active, "\nold body\n");
        let cmd = UpdatePendingWorkItem {
            title: Some("New Title".to_string()),
            prompt: Some("fresh prompt".to_string()),
            ..empty("GLP-0001")
        };

        let updated = super::execute(cmd, &store, &registry()).unwrap();

        assert_eq!(
            updated,
            UpdatePendingWorkItemOk::OpenItemEdit {
                id: "GLP-0001".to_string(),
                project: "glep-shimeji".to_string(),
                title: "new title".to_string(),
                title_normalized: false,
            }
        );
        let item = &store.items("glep-shimeji")[0];
        assert_eq!(item.title, "new title");
        assert_eq!(item.body, format!("## Goals{S}- fresh prompt"));
    }

    #[test]
    fn update_normalizes_yaml_breaking_title() {
        let store = staged(WorkItemStatus::Active, "body\n");
        let cmd = UpdatePendingWorkItem {
            title: Some("Fix Parser: Handle Colons".to_string()),
            ..empty("GLP-0001")
        };

        let updated = super::execute(cmd, &store, &registry()).unwrap();

        assert_eq!(
            store.items("glep-shimeji")[0].title,
            "fix parser; handle colons"
        );
        assert!(matches!(
            updated,
            UpdatePendingWorkItemOk::OpenItemEdit {
                title_normalized: true,
                ..
            }
        ));
    }

    fn staged_with_tags(raw: &str) -> InMemoryStore {
        let record = PendingWorkItem {
            tags: Some(raw.to_string()),
            ..record("GLP-0001", WorkItemStatus::Active, "body\n")
        };
        InMemoryStore::default()
            .with_prefix("glep-shimeji", "GLP")
            .with_project("glep-shimeji", vec![record])
    }

    fn tags(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn update_merges_tags_with_existing_frontmatter() {
        let store = staged_with_tags("[sqlite, godot]");
        let cmd = UpdatePendingWorkItem {
            tags: tags(&["godot", "csharp-export"]),
            ..empty("GLP-0001")
        };

        super::execute(cmd, &store, &registry()).unwrap();

        assert_eq!(
            store.items("glep-shimeji")[0].tags.as_deref(),
            Some("[sqlite, godot, csharp_export]")
        );
    }

    #[test]
    fn update_tags_clear_plus_tags_replaces_without_parsing_existing() {
        let store = staged_with_tags("still-corrupt");
        let cmd = UpdatePendingWorkItem {
            tags: tags(&["sqlite"]),
            tags_clear: true,
            ..empty("GLP-0001")
        };

        super::execute(cmd, &store, &registry()).unwrap();

        assert_eq!(
            store.items("glep-shimeji")[0].tags.as_deref(),
            Some("[sqlite]")
        );
    }

    #[test]
    fn update_rejects_corrupt_existing_tags_on_the_merge_path() {
        let store = staged_with_tags("sqlite, godot");
        let cmd = UpdatePendingWorkItem {
            tags: tags(&["sqlite"]),
            ..empty("GLP-0001")
        };

        let error = super::execute(cmd, &store, &registry()).unwrap_err();

        assert!(matches!(
            error,
            UpdatePendingWorkError::InvalidTagsFrontmatter { ref id, ref raw }
                if id == "GLP-0001" && raw == "sqlite, godot"
        ));
    }

    #[test]
    fn update_rejects_invalid_raw_tag_with_cli_display() {
        let store = staged(WorkItemStatus::Active, "body\n");
        let cmd = UpdatePendingWorkItem {
            tags: vec!["sqlite__export".to_string()],
            ..empty("GLP-0001")
        };

        let error = super::execute(cmd, &store, &registry()).unwrap_err();

        assert!(matches!(
            error,
            UpdatePendingWorkError::InvalidTag { ref raw } if raw == "sqlite__export"
        ));
        assert_eq!(
            error.to_string(),
            "Invalid --tag value \"sqlite__export\"; use lowercase/uppercase ASCII letters, digits, '_' or '-', without leading, trailing, or repeated separators."
        );
    }

    #[test]
    fn update_validates_and_merges_prerequisites_in_the_application() {
        let store = InMemoryStore::default()
            .with_prefix("glep-shimeji", "GLP")
            .with_project(
                "glep-shimeji",
                vec![
                    PendingWorkItem {
                        prereq: Some("[[GLP-0001]], [[GLP-0001]]".to_string()),
                        ..record("GLP-0002", WorkItemStatus::Active, "body\n")
                    },
                    record("GLP-0001", WorkItemStatus::Done, "body\n"),
                ],
            );
        let cmd = UpdatePendingWorkItem {
            prereq: vec!["glp1, GLP-0001".to_string()],
            ..empty("GLP-0002")
        };

        super::execute(cmd, &store, &registry()).unwrap();

        let item = store
            .items("glep-shimeji")
            .into_iter()
            .find(|item| {
                item.id
                    .as_item()
                    .is_some_and(|id| id.as_ref() == "GLP-0002")
            })
            .unwrap();
        assert_eq!(item.prereq.as_deref(), Some("[[GLP-0001]]"));
    }

    #[test]
    fn update_missing_item_preserves_requested_id() {
        let store = staged(WorkItemStatus::Active, "body\n");
        let cmd = UpdatePendingWorkItem {
            prompt: Some("x".to_string()),
            ..empty("glp-9999")
        };

        let error = super::execute(cmd, &store, &registry()).unwrap_err();

        assert!(matches!(
            error,
            UpdatePendingWorkError::ItemNotFound { ref id } if id == "glp-9999"
        ));
    }
}
