use thiserror::Error;

/// Carries pending-work errors across CLI and in-process handoff seams.
/// The handoff seam converts these errors with `to_string()` instead of matching variants.
#[derive(Debug, Error)]
pub(crate) enum PendingWorkError {
    #[error("{0}")]
    Config(
        #[from]
        #[source]
        crate::config::ConfigError,
    ),
    #[error("missing --config-path")]
    MissingConfigPath,
    #[error("{0}")]
    ApplicationList(String),
    #[error("{0}")]
    ApplicationRead(String),
    #[error("{0}")]
    ApplicationWrite(String),
    #[error(transparent)]
    SessionDispatch(#[from] pwf_application::pending_work::session::dispatch::DispatchSessionError),
    #[error(transparent)]
    SessionVerify(#[from] pwf_application::pending_work::session::verify::VerifySessionError),
    #[error("a pw subcommand is required.")]
    MissingSubcommand,
    #[error("Unknown action: {action}")]
    UnknownAction { action: String },
    #[error("Unknown --section value '{value}'. Use one of: future, human, low-prio.")]
    BadSection { value: String },
    #[error("Choose only one list scope flag: --human, --future, or --all.")]
    ConflictingListScopes,
    #[error(
        "--order field conflict: choose either 'created' or 'id', not both ('{first}' and '{second}')."
    )]
    ConflictingOrderField { first: String, second: String },
    #[error(
        "--order direction conflict: choose either 'asc' or 'desc', not both ('{first}' and '{second}')."
    )]
    ConflictingOrderDirection { first: String, second: String },
    #[error("Unknown --order value '{value}'. Use one of: created, id, asc, desc.")]
    BadOrderValue { value: String },
    #[error("Project '{project}' is not mapped to a repo in config/pending-work.json.")]
    ProjectNotMappedToRepo { project: String },
    #[error(
        "'{identifier}' is ambiguous. Managed project identifiers matching it: {}.",
        matches.join(", ")
    )]
    AmbiguousManagedProject {
        identifier: String,
        matches: Vec<String>,
    },
    #[error(
        "Unknown managed project identifier: {identifier}\nManaged project identifiers: {}",
        known.join(", ")
    )]
    UnknownManagedProject {
        identifier: String,
        known: Vec<String>,
    },
    #[error("Notes directory not found: {path}")]
    NotesDirectoryNotFound { path: String },
    #[error("Open pending-work item not found: {id}")]
    ItemNotFound { id: String },
    #[error("Pending-work id is ambiguous: {id}")]
    AmbiguousId { id: String },
    #[error("--id is required for {action}.")]
    MissingId { action: &'static str },
    #[error("--report cannot be empty.")]
    EmptyReport,
    #[error("--report is required for cancel.")]
    MissingCancelReport,
    #[error("remove only supports file-model pending-work items.")]
    RemoveRequiresFileModel,
    #[error("Work-item note missing: {}", path.display())]
    WorkItemNoteMissing { path: std::path::PathBuf },
    #[error(
        "nothing to update (pass --prompt, --title, --prereq, --clear-prereq, --tag, --tags-clear, --commits, --append-report, --append, and/or --effort)."
    )]
    NothingToUpdate,
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
    #[error("{}", ADD_HINT)]
    AddUsage,
    #[error("No handoff directory found for project at {}.", path.display())]
    NoHandoffDirectory { path: std::path::PathBuf },
    #[error("No handoff Markdown files found in {}.", path.display())]
    NoHandoffMarkdown { path: std::path::PathBuf },
    #[error("{source}")]
    ReadHandoffDirectory {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
    #[error("{}", ADD_HINT)]
    RouteCreateRejected,
    #[error(transparent)]
    Clean(#[from] crate::engines::clean::CleanError),
    #[error("Expected open task marker at {note}:{line}. The note may have changed.")]
    ExpectedOpenTaskMarker { note: String, line: usize },
    /// Reports a handoff preflight failure before any pending-work mutation.
    #[error(transparent)]
    HandoffMirror(#[from] crate::engines::handoff::mirror::MirrorError),
    /// Reports a successful pending-work mutation followed by a failed handoff mirror.
    /// Each producer supplies recovery steps for its post-mutation state.
    #[error("{id} was mutated, but its handoff was not: {source}\n  {remedy}")]
    HandoffMirrorAfterMutation {
        id: String,
        source: crate::engines::handoff::mirror::MirrorError,
        remedy: String,
    },
}

pub(crate) fn done_cancel_reopen_remedy(id: &str) -> String {
    format!(
        "fix the cause, then `pwf reopen --id {id}` and re-run — or finish the handoff move by hand"
    )
}

pub(crate) const REMOVE_MIRROR_REMEDY: &str =
    "the pw note is already deleted; delete the linked handoff file by hand";

pub(crate) const ADD_MIRROR_REMEDY: &str = "the item was created but its handoff scaffold failed; \
     create the handoff manually or remove the `handoff` tag";

impl From<PendingWorkError> for String {
    fn from(error: PendingWorkError) -> Self {
        error.to_string()
    }
}

pub(super) const ADD_HINT: &str = r#"Use: pwf add <project> "<prompt>""#;

#[cfg(test)]
mod tests {
    use std::{assert_matches, error::Error};

    use super::*;

    fn stub_mirror_error() -> crate::engines::handoff::mirror::MirrorError {
        crate::engines::handoff::mirror::MirrorError::HandoffNotFound {
            id: "GLP-0001".to_string(),
            dir: std::path::PathBuf::from("/repo/docs/handoffs"),
        }
    }

    #[test]
    fn handoff_mirror_after_mutation_remedy_differs_by_producer() {
        let done_like = PendingWorkError::HandoffMirrorAfterMutation {
            id: "GLP-0001".to_string(),
            source: stub_mirror_error(),
            remedy: done_cancel_reopen_remedy("GLP-0001"),
        };
        assert!(done_like.to_string().contains("pwf reopen --id GLP-0001"));

        let remove_like = PendingWorkError::HandoffMirrorAfterMutation {
            id: "GLP-0001".to_string(),
            source: stub_mirror_error(),
            remedy: REMOVE_MIRROR_REMEDY.to_string(),
        };
        let remove_message = remove_like.to_string();
        assert!(
            remove_message.contains("already deleted"),
            "got: {remove_message}"
        );
        assert!(
            !remove_message.contains("pwf reopen"),
            "remove has nothing to reopen: {remove_message}"
        );

        let add_like = PendingWorkError::HandoffMirrorAfterMutation {
            id: "GLP-0001".to_string(),
            source: stub_mirror_error(),
            remedy: ADD_MIRROR_REMEDY.to_string(),
        };
        let add_message = add_like.to_string();
        assert!(
            add_message.contains("create the handoff manually"),
            "got: {add_message}"
        );
        assert!(
            !add_message.contains("pwf reopen"),
            "a freshly created item has nothing to reopen: {add_message}"
        );
    }

    #[test]
    fn clean_error_bridge_preserves_display_and_source() {
        let err = PendingWorkError::from(crate::engines::clean::CleanError::ReadIndex {
            path: std::path::PathBuf::from("/tmp/index.md"),
            source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "locked"),
        });

        assert_matches!(err, PendingWorkError::Clean(_));
        assert_eq!(err.to_string(), "Cannot read index: locked");
        assert_eq!(err.source().unwrap().to_string(), "locked");
    }
}
