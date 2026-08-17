use std::path::PathBuf;

pub fn repo_root() -> PathBuf {
    let mut manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR")
        .map_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")), PathBuf::from);
    manifest_dir.pop();
    manifest_dir
}
