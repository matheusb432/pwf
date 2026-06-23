// Shared, interpolated error messages used across more than one submodule.
// Plain (non-interpolated) messages local to a single module live as a `const`
// in that module instead.

use thiserror::Error;

/// Typed errors from the pending-work engine internals.
#[derive(Debug, Error)]
pub(super) enum PendingWorkError {
    /// Config loading failed.
    #[error("{0}")]
    Config(
        #[from]
        #[source]
        crate::config::ConfigError,
    ),
    /// No config path was provided and no default could be resolved.
    #[error("missing --config-path")]
    MissingConfigPath,
    /// Store I/O failed.
    #[error(transparent)]
    Store(#[from] crate::engines::pending_work::obsidian::store::StoreError),
    /// The caller did not provide a pending-work subcommand.
    #[error("a pw subcommand is required.")]
    MissingSubcommand,
    /// The provided pending-work subcommand is not known.
    #[error("Unknown action: {action}")]
    UnknownAction {
        /// The action token after normalization.
        action: String,
    },
    /// The provided add section is not known.
    #[error("Unknown --section value '{value}'. Use one of: future, human, low-prio.")]
    BadSection {
        /// The raw section flag value.
        value: String,
    },
    /// More than one list scope flag was supplied.
    #[error("Choose only one list scope flag: --human, --future, or --all.")]
    ConflictingListScopes,
    /// A managed project has no configured repository path.
    #[error("Project '{project}' is not mapped to a repo in config/pending-work.json.")]
    ProjectNotMappedToRepo {
        /// The managed project name.
        project: String,
    },
    /// A managed project has no configured work-item prefix.
    #[error("Project '{project}' has no work-item prefix in config/pending-work.json (prefixes).")]
    ProjectMissingPrefix {
        /// The managed project name.
        project: String,
    },
    /// A managed project identifier matches multiple projects.
    #[error(
        "'{identifier}' is ambiguous. Managed project identifiers matching it: {}.",
        matches.join(", ")
    )]
    AmbiguousManagedProject {
        /// The raw identifier supplied by the caller.
        identifier: String,
        /// Matching managed project names, in config iteration order.
        matches: Vec<String>,
    },
    /// A managed project identifier matches no project.
    #[error(
        "Unknown managed project identifier: {identifier}\nManaged project identifiers: {}",
        known.join(", ")
    )]
    UnknownManagedProject {
        /// The raw identifier supplied by the caller.
        identifier: String,
        /// Known managed project names, in config iteration order.
        known: Vec<String>,
    },
    /// The configured notes directory could not be found.
    #[error("Notes directory not found: {path}")]
    NotesDirectoryNotFound {
        /// The configured notes directory path.
        path: String,
    },
    /// No open item matched the requested pending-work id.
    #[error("Open pending-work item not found: {id}")]
    ItemNotFound {
        /// The id requested by the caller.
        id: String,
    },
    /// More than one open item matched the requested pending-work id.
    #[error("Pending-work id is ambiguous: {id}")]
    AmbiguousId {
        /// The id requested by the caller.
        id: String,
    },
    /// The selected item cannot be launched until blocking issues are fixed.
    #[error("Pending-work item '{id}' is not launchable: {}", issues.join("; "))]
    NotLaunchable {
        /// The selected item id.
        id: String,
        /// Human-facing blocking launch issues.
        issues: Vec<String>,
    },
    /// An action requiring an id was invoked without one.
    #[error("--id is required for {action}.")]
    MissingId {
        /// The pending-work action name.
        action: &'static str,
    },
    /// A check report was supplied but contained no content.
    #[error("--report cannot be empty.")]
    EmptyReport,
    /// Cancel requires a report explaining what was tried and why work stopped.
    #[error("--report is required for cancel.")]
    MissingCancelReport,
    /// Remove only supports the file-model pending-work format.
    #[error("remove only supports file-model pending-work items.")]
    RemoveRequiresFileModel,
    /// Update only supports the file-model pending-work format.
    #[error("update only supports file-model pending-work items.")]
    UpdateRequiresFileModel,
    /// The item link existed, but its backing note file was absent.
    #[error("Work-item note missing: {}", path.display())]
    WorkItemNoteMissing {
        /// Missing work-item note path.
        path: std::path::PathBuf,
    },
    /// The index no longer contains the selected item link.
    #[error("Index link not found for {id}.")]
    IndexLinkNotFound {
        /// The selected item id.
        id: String,
    },
    /// Update was invoked without any field mutations.
    #[error(
        "nothing to update (pass --prompt, --title, --prereq, --clear-prereq, and/or --commits)."
    )]
    NothingToUpdate,
    /// A closed (done/cancelled) item only supports `--commits` amendment.
    #[error(
        "only --commits can amend closed item {id} (done/cancelled); body/title/prereq need an open item."
    )]
    ClosedItemCommitsOnly {
        /// The selected item id.
        id: String,
    },
    /// A prereq flag value is not a canonical work-item id.
    #[error("Invalid --prereq id: {raw}.")]
    InvalidPrereqId {
        /// The raw id token supplied by the caller.
        raw: String,
    },
    /// A prereq flag was supplied without any id tokens.
    #[error("--prereq requires an id.")]
    MissingPrereqId,
    /// One or more canonical prereq ids do not exist.
    #[error("Unknown --prereq id(s): {}.", ids.join(", "))]
    UnknownPrereqIds {
        /// Missing canonical ids, in caller order after deduplication.
        ids: Vec<String>,
    },
    /// Add was invoked without the positional project/prompt form.
    #[error("{}", ADD_HINT)]
    AddUsage,
    /// `--continue-handoff` could not find a handoff directory.
    #[error("No handoff directory found for project at {}.", path.display())]
    NoHandoffDirectory {
        /// Expected handoff directory path.
        path: std::path::PathBuf,
    },
    /// `--continue-handoff` found no usable Markdown handoff files.
    #[error("No handoff Markdown files found in {}.", path.display())]
    NoHandoffMarkdown {
        /// Handoff directory path.
        path: std::path::PathBuf,
    },
    /// Reading the handoff directory failed.
    #[error("{source}")]
    ReadHandoffDirectory {
        /// Handoff directory path.
        path: std::path::PathBuf,
        /// The underlying filesystem error.
        source: std::io::Error,
    },
    /// Route attempted one of the removed create forms.
    #[error("{}", ADD_HINT)]
    RouteCreateRejected,
    /// The clean engine failed.
    #[error(transparent)]
    Clean(#[from] crate::engines::clean::CleanError),
    /// A legacy inline item no longer has the expected open checkbox marker.
    #[error("Expected open task marker at {note}:{line}. The note may have changed.")]
    ExpectedOpenTaskMarker {
        /// The note path containing the legacy task.
        note: String,
        /// The line reported for the legacy task.
        line: usize,
    },
    /// `zellij` is not installed / not on PATH.
    #[error("zellij not found on PATH; cannot dispatch a pwf session (Linux-only feature).")]
    ZellijNotFound,
    /// The resolved repo directory does not exist.
    #[error("Repo directory for project '{project}' does not exist: {path}")]
    RepoMissing {
        /// The managed project name.
        project: String,
        /// The missing repo path.
        path: String,
    },
    /// `new-tab` failed even after creating the session.
    #[error("Failed to dispatch into zellij session '{session}': {message}")]
    SessionDispatchFailed {
        /// The target session name.
        session: String,
        /// Underlying zellij error text.
        message: String,
    },
}

impl From<PendingWorkError> for String {
    fn from(error: PendingWorkError) -> Self {
        error.to_string()
    }
}

/// The canonical create form. Pointed at by the route guards that reject the
/// removed silent-create paths (PWF-0034).
pub(super) const ADD_HINT: &str = r#"Use: pwf add <project> "<prompt>""#;

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn clean_error_bridge_preserves_display_and_source() {
        let err = PendingWorkError::from(crate::engines::clean::CleanError::ReadIndex {
            path: std::path::PathBuf::from("/tmp/index.md"),
            source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "locked"),
        });

        assert!(matches!(err, PendingWorkError::Clean(_)));
        assert_eq!(err.to_string(), "Cannot read index: locked");
        assert_eq!(err.source().unwrap().to_string(), "locked");
    }
}
