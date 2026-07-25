use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPath {
    pub(super) path: PathBuf,
    pub(super) identity: RuntimePathIdentity,
}

impl ResolvedPath {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn identity(&self) -> &RuntimePathIdentity {
        &self.identity
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct RuntimePathIdentity(pub(super) OsString);

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RuntimePathError {
    #[error("path must not be empty")]
    Empty,
    #[error("path must not contain repeated separators")]
    RepeatedSeparator,
    #[error("path must not end with a separator")]
    TrailingSeparator,
    #[error("path must not contain `.` or `..` components")]
    DotComponent,
    #[error("drive-relative paths are not supported")]
    DriveRelative,
    #[error("home-relative path must contain only normal path components")]
    HomeRelativeComponent,
    #[cfg(windows)]
    #[error("path prefix is not supported")]
    UnsupportedPrefix,
    #[error("home directory must be absolute")]
    RelativeHome,
}

/// Requests portable resolution of one managed-project path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveRuntimePath {
    /// Persisted path value.
    pub path: String,
    /// Home directory used for home-relative paths.
    pub home: PathBuf,
}

/// Resolves one managed-project path for runtime use.
///
/// # Errors
///
/// Returns [`RuntimePathError`] when the path or home directory violates the portable path
/// contract.
#[cqrsy::query]
pub fn execute(query: &ResolveRuntimePath) -> Result<ResolvedPath, RuntimePathError> {
    super::runtime_path::resolve(&query.path, &query.home)
}
