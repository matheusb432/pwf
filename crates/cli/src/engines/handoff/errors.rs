//! The handoff engine's error type, plus the best-effort-read status used by
//! reads that degrade (rather than fail) on a partially unreadable directory.

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub(super) enum HandoffError {
    #[error("--repo-root does not exist: {root}")]
    RepoRootDoesNotExist { root: String },
    #[error("{source}")]
    CurrentDir { source: std::io::Error },
    #[error("--title is required for new.")]
    MissingTitle,
    #[error("--id is required for {action}.")]
    MissingId { action: String },
    #[error("a handoff subcommand is required.")]
    MissingSubcommand,
    #[error("unknown handoff action: {action}")]
    UnknownAction { action: String },
    #[error("Handoff already exists: {}", path.display())]
    HandoffAlreadyExists { path: PathBuf },
    #[error("No active handoff found for '{key}' in {}.", dir.display())]
    ActiveHandoffNotFound { key: String, dir: PathBuf },
    #[error("No archived handoff found for '{key}' in {}.", dir.display())]
    ArchivedHandoffNotFound { key: String, dir: PathBuf },
    #[error("Archived handoff already exists: {}", path.display())]
    ArchiveAlreadyExists { path: PathBuf },
    #[error("Cannot remove active handoff after archiving: {source}")]
    RemoveActiveAfterArchive {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{source}")]
    CreateDir {
        action: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{source}")]
    Read {
        action: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{source}")]
    Write {
        action: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{source}")]
    Rename {
        action: &'static str,
        from: PathBuf,
        to: PathBuf,
        source: std::io::Error,
    },
    #[error("{message}")]
    PendingWork { message: String },
    #[error("pw-add output parse error (stdout: {stdout})")]
    PwAddParse { stdout: String },
    #[error("{source}")]
    SubprocessSpawn {
        operation: &'static str,
        script: String,
        source: std::io::Error,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HandoffReadStatus {
    Complete,
    Degraded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct HandoffRead<T> {
    pub(super) status: HandoffReadStatus,
    pub(super) value: T,
}

impl<T> HandoffRead<T> {
    pub(super) fn complete(value: T) -> Self {
        Self {
            status: HandoffReadStatus::Complete,
            value,
        }
    }

    pub(super) fn degraded(value: T) -> Self {
        Self {
            status: HandoffReadStatus::Degraded,
            value,
        }
    }
}
