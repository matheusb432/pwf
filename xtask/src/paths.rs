//! Filesystem anchors for the host repo, resolved from this crate's compile-time location so
//! they are independent of the process cwd.

use std::path::{Path, PathBuf};

/// Absolute path to the xdb repo root (this crate lives at `<root>/xtask`).
///
/// Anchored on `CARGO_MANIFEST_DIR` rather than the cwd: `install`/`fmt` must locate the repo
/// no matter where `cargo run -p xtask` was invoked from.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
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
}
