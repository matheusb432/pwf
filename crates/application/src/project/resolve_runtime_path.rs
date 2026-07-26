use std::path::PathBuf;

pub use super::runtime_path::{ResolvedPath, RuntimePathError, RuntimePathIdentity};

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
