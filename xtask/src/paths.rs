//! Filesystem anchors for the host repo, resolved from the runtime `CARGO_MANIFEST_DIR` cargo
//! sets for `cargo run`/`cargo test`, so they are independent of the process cwd AND of which
//! checkout compiled the binary.

use std::path::{Path, PathBuf};

/// Absolute path to the pwf repo root (this crate lives at `<root>/xtask`).
///
/// Resolved from the **runtime** `CARGO_MANIFEST_DIR` env var, not the compile-time `env!`
/// constant: the machine-wide shared cargo build-dir (`~/.cargo/config.toml [build] build-dir`)
/// reuses artifacts across checkouts, so a compile-time path baked by a `.worktrees/<name>`
/// build can leak into the main checkout's binary and dangle once the worktree is removed
/// (it retargeted the global `pwf` shim to a deleted path). Cargo sets the var at runtime for
/// every `cargo run -p xtask`/`cargo test` invocation — always the invoking checkout. The
/// compile-time value remains only as a fallback for running the binary outside cargo.
pub fn repo_root() -> PathBuf {
    let manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR")
        .map_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")), PathBuf::from);
    root_from(&manifest_dir)
}

/// `<root>` from the xtask manifest dir `<root>/xtask`.
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
