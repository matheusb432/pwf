//! Repository paths resolved independently of the process working directory.

use std::path::PathBuf;

/// Returns the absolute repository root.
///
/// Runtime `CARGO_MANIFEST_DIR` identifies the invoking checkout even when Cargo shares build
/// artifacts across worktrees. The compile-time path is only a fallback outside Cargo.
pub fn repo_root() -> PathBuf {
    let mut manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR")
        .map_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")), PathBuf::from);
    manifest_dir.pop();
    manifest_dir
}
