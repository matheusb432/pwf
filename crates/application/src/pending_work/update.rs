use std::sync::LazyLock;

use pwf_domain::pending_work::{
    ParsePrereqsError, Prereqs, ProjectRegistry, Tags, UpdatedItem, WorkItemId, WorkItemStatus,
    append_lanes, append_report_block, normalize_title, note_body,
};
use regex::Regex;

use super::store_util::body_region;
use crate::ports::{AppDbStore, ItemPatch, PendingWorkItem};

/// Extracts bare ids from an existing `prereq` frontmatter value.
static PREREQ_VALUE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[([A-Z]{2,4}-\d{4})").expect("valid prereq regex"));

#[derive(Debug, Clone)]
pub struct UpdatePendingWorkItem {
    pub id: String,
    pub prompt: Option<String>,
    pub title: Option<String>,
    pub append: Option<String>,
    pub prereq: Vec<String>,
    pub clear_prereq: bool,
    pub commits: Option<String>,
    pub append_report: Option<String>,
    pub effort: Option<u8>,
    pub tags: Option<Tags>,
    pub tags_clear: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum UpdatePendingWorkError {
    #[error("Open pending-work item not found: {id}")]
    ItemNotFound { id: String },
    #[error(
        "nothing to update (pass --prompt, --title, --prereq, --clear-prereq, --tag, --tags-clear, --commits, --append-report, --append, and/or --effort)."
    )]
    NothingToUpdate,
    #[error(
        "only --commits / --append-report can amend closed item {id} (done/cancelled); body/title/prereq/tags/append/effort need an open item."
    )]
    ClosedItemAmendOnly { id: String },
    #[error("item {id} has invalid tags frontmatter: {raw:?}.")]
    InvalidTagsFrontmatter { id: String, raw: String },
    #[error("Invalid --prereq id: {raw}.")]
    InvalidPrereqId { raw: String },
    #[error("--prereq requires an id.")]
    MissingPrereqId,
    #[error("Unknown --prereq id(s): {}.", ids.join(", "))]
    UnknownPrereqIds { ids: Vec<String> },
    #[error("--report cannot be empty.")]
    EmptyReport,
    #[error("--append cannot be empty.")]
    EmptyAppend,
    #[error("{0}")]
    WriteStore(Box<dyn std::error::Error + Send + Sync>),
}

/// Applies body and frontmatter edits in one item patch.
///
/// Closed items accept only commit and report amendments.
#[cqrsy::command]
pub fn execute<S>(
    cmd: UpdatePendingWorkItem,
    store: &S,
    projects: &ProjectRegistry,
) -> Result<UpdatedItem, UpdatePendingWorkError>
where
    S: AppDbStore<PendingWorkItem>,
{
    let edits_body = cmd.prompt.is_some()
        || cmd.title.is_some()
        || !cmd.prereq.is_empty()
        || cmd.clear_prereq
        || cmd.append.is_some()
        || cmd.effort.is_some()
        || cmd.tags.is_some()
        || cmd.tags_clear;
    if !edits_body && cmd.commits.is_none() && cmd.append_report.is_none() {
        return Err(UpdatePendingWorkError::NothingToUpdate);
    }

    let not_found = || UpdatePendingWorkError::ItemNotFound { id: cmd.id.clone() };
    let id = WorkItemId::try_new(&cmd.id).map_err(|_| not_found())?;
    let project = projects.project_for_id(&id).ok_or_else(not_found)?;
    let record = store
        .get(project, &id)
        .map_err(|error| UpdatePendingWorkError::WriteStore(Box::new(error)))?
        .ok_or_else(not_found)?;

    if record.status == WorkItemStatus::Active {
        edit_open_item(store, projects, &cmd, &id, &record)
    } else if edits_body {
        Err(UpdatePendingWorkError::ClosedItemAmendOnly { id: cmd.id })
    } else {
        amend_closed_item(store, project, &cmd, &id, &record)
    }
}

fn edit_open_item<S>(
    store: &S,
    projects: &ProjectRegistry,
    cmd: &UpdatePendingWorkItem,
    id: &WorkItemId,
    record: &PendingWorkItem,
) -> Result<UpdatedItem, UpdatePendingWorkError>
where
    S: AppDbStore<PendingWorkItem>,
{
    let new_title = cmd
        .title
        .as_deref()
        .map_or_else(|| record.title.clone(), normalize_title);

    let mut patch = ItemPatch {
        body: compute_body(cmd, record)?,
        ..ItemPatch::default()
    };
    if cmd.title.is_some() {
        patch.title = Some(new_title.clone());
    }
    if cmd.clear_prereq {
        patch.prereq = Some(None);
    } else if !cmd.prereq.is_empty() {
        let merged = merge_prereqs(store, projects, record.prereq.as_deref(), &cmd.prereq)?;
        patch.prereq = Some(Some(merged));
    }
    if let Some(commits) = &cmd.commits {
        patch.commits = Some(Some(commits.clone()));
    }
    if let Some(effort) = cmd.effort {
        patch.effort = Some(effort);
    }
    patch.tags = resolve_tags(cmd, id, record)?;

    let project = projects.project_for_id(id).expect("id already resolved");
    store
        .update(project, id, patch)
        .map_err(|error| UpdatePendingWorkError::WriteStore(Box::new(error)))?;
    Ok(UpdatedItem::OpenItemEdit {
        id: id.as_ref().to_string(),
        project: project.as_ref().to_string(),
        title: new_title,
    })
}

/// Computes a replacement body when a prompt, lane, or report edit requires one.
///
/// Prompt replacement starts from a new body; lane and report edits build on that result.
fn compute_body(
    cmd: &UpdatePendingWorkItem,
    record: &PendingWorkItem,
) -> Result<Option<String>, UpdatePendingWorkError> {
    let base = body_region(&record.body);
    let mut body: Option<String> = cmd.prompt.as_deref().map(note_body);
    if let Some(append) = &cmd.append {
        let current = body.as_deref().unwrap_or(base);
        body = Some(append_lanes(current, append).ok_or(UpdatePendingWorkError::EmptyAppend)?);
    }
    if let Some(report) = &cmd.append_report {
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
    cmd: &UpdatePendingWorkItem,
    id: &WorkItemId,
    record: &PendingWorkItem,
) -> Result<Option<Option<Tags>>, UpdatePendingWorkError> {
    let Some(appended) = &cmd.tags else {
        return Ok(cmd.tags_clear.then_some(None));
    };
    let tags = if cmd.tags_clear {
        appended.clone()
    } else if let Some(existing) = record.tags.as_deref() {
        Tags::parse_frontmatter(existing)
            .map_err(|error| UpdatePendingWorkError::InvalidTagsFrontmatter {
                id: id.as_ref().to_string(),
                raw: error.raw().to_string(),
            })?
            .merged(appended)
    } else {
        appended.clone()
    };
    Ok(Some(Some(tags)))
}

fn amend_closed_item<S>(
    store: &S,
    project: &pwf_domain::pending_work::ProjectName,
    cmd: &UpdatePendingWorkItem,
    id: &WorkItemId,
    record: &PendingWorkItem,
) -> Result<UpdatedItem, UpdatePendingWorkError>
where
    S: AppDbStore<PendingWorkItem>,
{
    let mut patch = ItemPatch::default();
    let mut changes = Vec::new();
    if let Some(commits) = &cmd.commits {
        patch.commits = Some(Some(commits.clone()));
        changes.push(format!("commits: {commits}"));
    }
    if let Some(report) = &cmd.append_report {
        let base = body_region(&record.body);
        patch.body =
            Some(append_report_block(base, report).ok_or(UpdatePendingWorkError::EmptyReport)?);
        changes.push("report appended".to_string());
    }
    store
        .update(project, id, patch)
        .map_err(|error| UpdatePendingWorkError::WriteStore(Box::new(error)))?;
    Ok(UpdatedItem::Changed {
        id: id.as_ref().to_string(),
        changes,
    })
}

/// Validates and deduplicates prerequisite ids before appending them to frontmatter.
fn merge_prereqs<S>(
    store: &S,
    projects: &ProjectRegistry,
    existing: Option<&str>,
    values: &[String],
) -> Result<String, UpdatePendingWorkError>
where
    S: AppDbStore<PendingWorkItem>,
{
    let mut ids: Vec<String> = existing
        .into_iter()
        .flat_map(|value| {
            PREREQ_VALUE_RE
                .captures_iter(value)
                .map(|captures| captures[1].to_string())
        })
        .collect();
    let prereqs = Prereqs::parse_values(values).map_err(map_parse_prereqs_error)?;
    let mut missing = Vec::new();
    for id in prereqs.iter() {
        if !prereq_exists(store, projects, id)? {
            missing.push(id.as_ref().to_string());
        }
    }
    if !missing.is_empty() {
        return Err(UpdatePendingWorkError::UnknownPrereqIds { ids: missing });
    }
    for id in prereqs.ids() {
        if !ids.iter().any(|existing| existing == id) {
            ids.push(id.to_string());
        }
    }
    Ok(ids
        .iter()
        .map(|id| format!("[[{id}]]"))
        .collect::<Vec<_>>()
        .join(", "))
}

fn prereq_exists<S>(
    store: &S,
    projects: &ProjectRegistry,
    id: &WorkItemId,
) -> Result<bool, UpdatePendingWorkError>
where
    S: AppDbStore<PendingWorkItem>,
{
    let Some(project) = projects.project_for_id(id) else {
        return Ok(false);
    };
    Ok(store
        .get(project, id)
        .map_err(|error| UpdatePendingWorkError::WriteStore(Box::new(error)))?
        .is_some())
}

fn map_parse_prereqs_error(error: ParsePrereqsError) -> UpdatePendingWorkError {
    match error {
        ParsePrereqsError::MissingId => UpdatePendingWorkError::MissingPrereqId,
        ParsePrereqsError::InvalidId { raw } => UpdatePendingWorkError::InvalidPrereqId { raw },
    }
}

#[cfg(test)]
mod tests {
    use pwf_domain::pending_work::{
        ProjectName, ProjectRegistry, Tags, Timestamp, UpdatedItem, WorkItemId, WorkItemStatus,
    };

    use super::{UpdatePendingWorkError, UpdatePendingWorkItem, execute};
    use crate::{Materialization, PendingWorkItem, RecordId, testing::InMemoryStore};

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
            source: body.to_string(),
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
            commits: None,
            append_report: None,
            effort: None,
            tags: None,
            tags_clear: false,
        }
    }

    #[test]
    fn update_rejects_empty_patch_with_nothing_to_update() {
        let store = staged(WorkItemStatus::Active, "## Goals\n- x\n");

        let error = execute(empty("GLP-0001"), &store, &registry()).unwrap_err();

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
            commits: Some("abc..def".to_string()),
            ..empty("GLP-0001")
        };

        let updated = execute(cmd, &store, &registry()).unwrap();

        assert_eq!(
            updated,
            UpdatedItem::Changed {
                id: "GLP-0001".to_string(),
                changes: vec!["commits: abc..def".to_string()],
            }
        );
        assert_eq!(
            store.items("glep-shimeji")[0].commits.as_deref(),
            Some("abc..def")
        );
    }

    #[test]
    fn update_rejects_body_edit_on_closed_item() {
        let store = staged(WorkItemStatus::Done, "body\n");
        let cmd = UpdatePendingWorkItem {
            title: Some("new title".to_string()),
            ..empty("GLP-0001")
        };

        let error = execute(cmd, &store, &registry()).unwrap_err();

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

        let updated = execute(cmd, &store, &registry()).unwrap();

        assert_eq!(
            updated,
            UpdatedItem::OpenItemEdit {
                id: "GLP-0001".to_string(),
                project: "glep-shimeji".to_string(),
                title: "new title".to_string(),
            }
        );
        let item = &store.items("glep-shimeji")[0];
        assert_eq!(item.title, "new title");
        assert_eq!(item.body, "## Goals\n- fresh prompt");
    }

    #[test]
    fn update_normalizes_yaml_breaking_title() {
        let store = staged(WorkItemStatus::Active, "body\n");
        let cmd = UpdatePendingWorkItem {
            title: Some("Fix Parser: Handle Colons".to_string()),
            ..empty("GLP-0001")
        };

        execute(cmd, &store, &registry()).unwrap();

        assert_eq!(
            store.items("glep-shimeji")[0].title,
            "fix parser; handle colons"
        );
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

    fn tags(values: &[&str]) -> Tags {
        Tags::parse_values(&values.iter().map(|v| (*v).to_string()).collect::<Vec<_>>()).unwrap()
    }

    #[test]
    fn update_merges_tags_with_existing_frontmatter() {
        let store = staged_with_tags("[sqlite, godot]");
        let cmd = UpdatePendingWorkItem {
            tags: Some(tags(&["godot", "csharp-export"])),
            ..empty("GLP-0001")
        };

        execute(cmd, &store, &registry()).unwrap();

        assert_eq!(
            store.items("glep-shimeji")[0].tags.as_deref(),
            Some("[sqlite, godot, csharp_export]")
        );
    }

    #[test]
    fn update_tags_clear_plus_tags_replaces_without_parsing_existing() {
        let store = staged_with_tags("still-corrupt");
        let cmd = UpdatePendingWorkItem {
            tags: Some(tags(&["sqlite"])),
            tags_clear: true,
            ..empty("GLP-0001")
        };

        execute(cmd, &store, &registry()).unwrap();

        assert_eq!(
            store.items("glep-shimeji")[0].tags.as_deref(),
            Some("[sqlite]")
        );
    }

    #[test]
    fn update_rejects_corrupt_existing_tags_on_the_merge_path() {
        let store = staged_with_tags("sqlite, godot");
        let cmd = UpdatePendingWorkItem {
            tags: Some(tags(&["sqlite"])),
            ..empty("GLP-0001")
        };

        let error = execute(cmd, &store, &registry()).unwrap_err();

        assert!(matches!(
            error,
            UpdatePendingWorkError::InvalidTagsFrontmatter { ref id, ref raw }
                if id == "GLP-0001" && raw == "sqlite, godot"
        ));
    }

    #[test]
    fn update_missing_item_preserves_requested_id() {
        let store = staged(WorkItemStatus::Active, "body\n");
        let cmd = UpdatePendingWorkItem {
            prompt: Some("x".to_string()),
            ..empty("glp-9999")
        };

        let error = execute(cmd, &store, &registry()).unwrap_err();

        assert!(matches!(
            error,
            UpdatePendingWorkError::ItemNotFound { ref id } if id == "glp-9999"
        ));
    }
}
