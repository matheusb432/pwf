use std::path::PathBuf;

pub mod add_handoff;
mod ledger;
pub(crate) mod lifecycle;
pub mod list_handoffs;
mod naming;
pub mod ports;

/// Describes the linked handoff side effect of a pending-work mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandoffMutationOk {
    /// The pending-work item is not linked to a handoff.
    NotLinked,
    /// A linked handoff was created.
    Created { path: PathBuf },
    /// A linked handoff was archived.
    Archived { path: PathBuf },
    /// A linked handoff was restored to the active directory.
    Reopened { path: PathBuf },
    /// A linked handoff was removed.
    Removed { path: PathBuf },
    /// The handoff already had the requested lifecycle state.
    AlreadyInTargetState,
}

/// Reports handoff validation, persistence, and recovery failures.
#[derive(Debug, thiserror::Error)]
pub enum HandoffError {
    /// Reading the pending-work record used by the handoff gate failed.
    #[error("{source}")]
    ReadPendingWork {
        /// Pending-work lookup failure retained as the source.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// A handoff-tagged pending-work record has malformed tags.
    #[error("item {id} has invalid tags frontmatter: {raw}")]
    InvalidTags { id: String, raw: String },
    /// A handoff-tagged item belongs to a managed project without a directory-backed repository.
    #[error(
        "item {id} is tagged `handoff` but the managed project record for {project} has no directory source for its repository; inspect it with `pwf project ls`"
    )]
    UnmanagedProject { id: String, project: String },
    /// A handoff-tagged item maps to a repository that is absent.
    #[error("item {id} is tagged `handoff` but its repo path does not exist: {}", path.display())]
    RepositoryMissing { id: String, path: PathBuf },
    /// A handoff continuation requires an active handoff directory.
    #[error("No handoff directory found for project at {}.", path.display())]
    HandoffDirectoryMissing { path: PathBuf },
    /// A handoff continuation requires at least one Markdown document.
    #[error("No handoff Markdown files found in {}.", path.display())]
    HandoffMarkdownMissing { path: PathBuf },
    /// No handoff in the expected directory links the pending-work item.
    #[error(
        "item {id} is tagged `handoff` but no handoff with `pw: {id}` exists in {} — untag it (`pwf update --id {id} --tags-clear`) or create the handoff",
        directory.display()
    )]
    HandoffNotFound { id: String, directory: PathBuf },
    /// Several handoffs in one directory link the same pending-work item.
    #[error(
        "more than one handoff in {} claims `pw: {id}` — fix the duplicate frontmatter",
        directory.display()
    )]
    AmbiguousHandoff { id: String, directory: PathBuf },
    /// The target active handoff path is already occupied.
    #[error("active handoff already exists: {}", path.display())]
    ActiveDestinationExists { path: PathBuf },
    /// The target archived handoff path is already occupied.
    #[error("archived handoff already exists: {}", path.display())]
    ArchivedDestinationExists { path: PathBuf },
    /// Reading handoff records failed.
    #[error("{source}")]
    ReadDocuments {
        path: PathBuf,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// Writing or moving one handoff record failed.
    #[error("{source}")]
    WriteDocument {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// Deleting one handoff record failed.
    #[error("{source}")]
    DeleteDocument {
        path: PathBuf,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    /// Rebuilding the derived handoff ledger failed.
    #[error("{source}")]
    RebuildLedger {
        path: PathBuf,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}
