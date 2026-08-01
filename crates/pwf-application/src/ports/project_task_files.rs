use std::{
    error::Error,
    path::{Path, PathBuf},
};

use pwf_models::project::ProjectIndexIdentity;

/// Reports the filesystem state after staged project task files commit.
#[derive(Debug)]
pub enum ProjectTaskFilesRenameCommit {
    /// The destination is installed and the source backup is removed.
    Complete,
    /// The destination is installed, but the source backup remains.
    BackupRetained {
        /// Retained backup directory.
        path: PathBuf,
        /// Backup removal failure.
        source: Box<dyn Error + Send + Sync>,
    },
}

/// Commits or discards one staged project task-file rename.
pub trait StagedProjectTaskFilesRename: Send + 'static {
    /// Concrete adapter failure.
    type Error: Error + Send + Sync + 'static;

    /// Installs the staged destination and removes the source.
    ///
    /// # Errors
    ///
    /// Returns the adapter error when the staged destination cannot be installed while preserving
    /// or restoring the source.
    fn commit(self) -> Result<ProjectTaskFilesRenameCommit, Self::Error>;

    /// Removes the staged destination without changing the source.
    ///
    /// # Errors
    ///
    /// Returns the adapter error when the staging directory cannot be removed.
    fn discard(self) -> Result<(), Self::Error>;
}

/// Stages project task-file relocation and identity rewriting.
pub trait ProjectTaskFilesClient: Clone + Send + Sync + 'static {
    /// Concrete adapter failure.
    type Error: Error + Send + Sync + 'static;
    /// Staged rename owned by this adapter.
    type StagedRename: StagedProjectTaskFilesRename<Error = Self::Error>;

    /// Copies and rewrites project task files without changing the source.
    ///
    /// # Errors
    ///
    /// Returns the adapter error when source validation, copying, or rewriting fails.
    fn stage_project_rename(
        &self,
        source: &Path,
        destination: &Path,
        current: &ProjectIndexIdentity,
        next: &ProjectIndexIdentity,
    ) -> Result<Self::StagedRename, Self::Error>;
}
