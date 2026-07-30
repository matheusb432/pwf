use pwf_models::pending_work::{EffortTier, TaskTitle};

use super::{
    ProjectRegistry,
    logic::pending_work_update::{persist, prepare},
};
use crate::ports::{AppRecordStore, PendingWorkRecord};

/// Requests edits to one pending-work item.
#[derive(Debug, Clone)]
pub struct UpdatePendingWorkItem {
    /// Requested work-item identifier.
    pub id: String,
    /// Optional replacement prompt.
    pub prompt: Option<String>,
    /// Optional replacement title.
    pub title: Option<TaskTitle>,
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
    pub effort: Option<EffortTier>,
    /// Raw tags appended to existing tags.
    pub tags: Vec<String>,
    /// Whether existing tags are cleared before applying raw tags.
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
    #[error(
        "Invalid --tag value {raw:?}; use lowercase/uppercase ASCII letters, digits, '_' or '-', without leading, trailing, or repeated separators."
    )]
    InvalidTag { raw: String },
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
    WriteStore(#[source] Box<dyn std::error::Error + Send + Sync>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdatePendingWorkItemOk {
    OpenItemEdit {
        id: String,
        project: String,
        title: String,
    },
    Changed {
        id: String,
        changes: Vec<String>,
    },
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
    S: AppRecordStore<PendingWorkRecord>,
{
    let prepared = prepare(&command, store, projects)?;
    persist(prepared, store)
}

#[cfg(test)]
mod tests {
    use pwf_models::pending_work::{ProjectName, TaskTitle, Timestamp, WorkItemId, WorkItemStatus};

    use super::{
        ProjectRegistry, UpdatePendingWorkError, UpdatePendingWorkItem, UpdatePendingWorkItemOk,
    };
    use crate::{Materialization, PendingWorkRecord, RecordId, testing::InMemoryStore};

    fn registry() -> ProjectRegistry {
        ProjectRegistry::new(vec![(
            ProjectName::try_new("foo-bar").unwrap(),
            Some("/repo".to_string()),
            Some("FOO".to_string()),
        )])
    }

    fn record(id: &str, status: WorkItemStatus, body: &str) -> PendingWorkRecord {
        PendingWorkRecord {
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
            locator: format!("/mem/foo-bar/{id}.md"),
            placement: None,
            materialization: Materialization::NoteFile,
        }
    }

    fn staged(status: WorkItemStatus, body: &str) -> InMemoryStore {
        InMemoryStore::default()
            .with_prefix("foo-bar", "FOO")
            .with_project("foo-bar", vec![record("FOO-0001", status, body)])
    }

    fn task_title(raw: &str) -> TaskTitle {
        TaskTitle::try_new(raw).unwrap()
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

        let error = super::execute(empty("FOO-0001"), &store, &registry()).unwrap_err();

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
            ..empty("FOO-0001")
        };

        let updated = super::execute(cmd, &store, &registry()).unwrap();

        assert_eq!(
            updated,
            UpdatePendingWorkItemOk::Changed {
                id: "FOO-0001".to_string(),
                changes: vec!["commits: abc..def, ghi..jkl".to_string()],
            }
        );
        assert_eq!(
            store.items("foo-bar")[0].commits.as_deref(),
            Some("abc..def, ghi..jkl")
        );
    }

    #[test]
    fn update_rejects_body_edit_on_closed_item() {
        let store = staged(WorkItemStatus::Done, "body\n");
        let cmd = UpdatePendingWorkItem {
            title: Some(task_title("new title")),
            ..empty("FOO-0001")
        };

        let error = super::execute(cmd, &store, &registry()).unwrap_err();

        assert!(matches!(
            error,
            UpdatePendingWorkError::ClosedItemAmendOnly { ref id } if id == "FOO-0001"
        ));
    }

    #[test]
    fn update_open_item_edits_title_and_body_and_reports_open_edit() {
        let store = staged(WorkItemStatus::Active, "\nold body\n");
        let cmd = UpdatePendingWorkItem {
            title: Some(task_title("New Title")),
            prompt: Some("fresh prompt".to_string()),
            ..empty("FOO-0001")
        };

        let updated = super::execute(cmd, &store, &registry()).unwrap();

        assert_eq!(
            updated,
            UpdatePendingWorkItemOk::OpenItemEdit {
                id: "FOO-0001".to_string(),
                project: "foo-bar".to_string(),
                title: "new title".to_string(),
            }
        );
        let item = &store.items("foo-bar")[0];
        assert_eq!(item.title, "new title");
        assert_eq!(item.body, "## Goals\n");
    }

    fn staged_with_tags(raw: &str) -> InMemoryStore {
        let record = PendingWorkRecord {
            tags: Some(raw.to_string()),
            ..record("FOO-0001", WorkItemStatus::Active, "body\n")
        };
        InMemoryStore::default()
            .with_prefix("foo-bar", "FOO")
            .with_project("foo-bar", vec![record])
    }

    fn tags(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn update_merges_tags_with_existing_frontmatter() {
        let store = staged_with_tags("[sqlite, godot]");
        let cmd = UpdatePendingWorkItem {
            tags: tags(&["godot", "csharp-export"]),
            ..empty("FOO-0001")
        };

        super::execute(cmd, &store, &registry()).unwrap();

        assert_eq!(
            store.items("foo-bar")[0].tags.as_deref(),
            Some("[sqlite, godot, csharp_export]")
        );
    }

    #[test]
    fn update_tags_clear_plus_tags_replaces_without_parsing_existing() {
        let store = staged_with_tags("still-corrupt");
        let cmd = UpdatePendingWorkItem {
            tags: tags(&["sqlite"]),
            tags_clear: true,
            ..empty("FOO-0001")
        };

        super::execute(cmd, &store, &registry()).unwrap();

        assert_eq!(store.items("foo-bar")[0].tags.as_deref(), Some("[sqlite]"));
    }

    #[test]
    fn update_rejects_corrupt_existing_tags_on_the_merge_path() {
        let store = staged_with_tags("sqlite, godot");
        let cmd = UpdatePendingWorkItem {
            tags: tags(&["sqlite"]),
            ..empty("FOO-0001")
        };

        let error = super::execute(cmd, &store, &registry()).unwrap_err();

        assert!(matches!(
            error,
            UpdatePendingWorkError::InvalidTagsFrontmatter { ref id, ref raw }
                if id == "FOO-0001" && raw == "sqlite, godot"
        ));
    }

    #[test]
    fn update_rejects_invalid_raw_tag_with_cli_display() {
        let store = staged(WorkItemStatus::Active, "body\n");
        let cmd = UpdatePendingWorkItem {
            tags: vec!["sqlite__export".to_string()],
            ..empty("FOO-0001")
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
            .with_prefix("foo-bar", "FOO")
            .with_project(
                "foo-bar",
                vec![
                    PendingWorkRecord {
                        prereq: Some("[[FOO-0001]], [[FOO-0001]]".to_string()),
                        ..record("FOO-0002", WorkItemStatus::Active, "body\n")
                    },
                    record("FOO-0001", WorkItemStatus::Done, "body\n"),
                ],
            );
        let cmd = UpdatePendingWorkItem {
            prereq: vec!["foo1, FOO-0001".to_string()],
            ..empty("FOO-0002")
        };

        super::execute(cmd, &store, &registry()).unwrap();

        let item = store
            .items("foo-bar")
            .into_iter()
            .find(|item| {
                item.id
                    .as_item()
                    .is_some_and(|id| id.as_ref() == "FOO-0002")
            })
            .unwrap();
        assert_eq!(item.prereq.as_deref(), Some("[[FOO-0001]]"));
    }

    #[test]
    fn update_missing_item_preserves_requested_id() {
        let store = staged(WorkItemStatus::Active, "body\n");
        let cmd = UpdatePendingWorkItem {
            prompt: Some("x".to_string()),
            ..empty("foo-9999")
        };

        let error = super::execute(cmd, &store, &registry()).unwrap_err();

        assert!(matches!(
            error,
            UpdatePendingWorkError::ItemNotFound { ref id } if id == "foo-9999"
        ));
    }
}
