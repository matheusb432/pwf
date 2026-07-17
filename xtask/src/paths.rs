//! Repository paths resolved independently of the process working directory.

use std::path::{Path, PathBuf};

/// Returns the absolute repository root.
///
/// Runtime `CARGO_MANIFEST_DIR` identifies the invoking checkout even when Cargo shares build
/// artifacts across worktrees. The compile-time path is only a fallback outside Cargo.
pub fn repo_root() -> PathBuf {
    let manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR")
        .map_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")), PathBuf::from);
    root_from(&manifest_dir)
}

/// Derives `<root>` from the `<root>/xtask` manifest directory.
fn root_from(manifest_dir: &Path) -> PathBuf {
    manifest_dir
        .parent()
        .expect("xtask always has a one-level parent")
        .to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_root_holds_the_workspace_manifest() {
        assert!(repo_root().join("Cargo.toml").is_file());
    }

    #[test]
    fn root_from_strips_the_xtask_leaf() {
        assert_eq!(
            root_from(Path::new("/some/checkout/xtask")),
            PathBuf::from("/some/checkout")
        );
    }
}
