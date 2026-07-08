// Date, id allocation, and path/key helpers — primitives now live in pwf-core;
// this module keeps the pending-work-specific archive scan + path helpers.

use std::path::Path;

pub use pwf_core::{
    date::stamp_date,
    paths::{project_dir, project_index_path, project_key},
};

pub(in crate::engines::pending_work) const ARCHIVE_DIR: &str = "_archive";

/// Convert a Path to a forward-slash string for output (portable across OSes).
pub(super) fn path_str(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// Allocates `KEY-{max+1:04}` by scanning `<dir>/KEY-*.md` and archived notes.
/// Gaps are preserved.
pub fn next_work_item_id(dir: &Path, key: &str) -> String {
    pwf_core::id::next_id(&[dir, &dir.join(ARCHIVE_DIR)], key)
}

/// Build a forward-slash relative path from `base` to `target`.
pub(super) fn pathdiff_forward(base: &str, target: &Path) -> String {
    // Use std::path for correct cross-platform relative computation, then
    // replace backslashes with forward slashes for the prompt string.
    let base_path = Path::new(base);
    // Simple approach: strip base prefix from target
    if let Ok(rel) = target.strip_prefix(base_path) {
        return rel.to_string_lossy().replace('\\', "/");
    }
    // Fallback: just the file name
    target
        .file_name()
        .map(|n| n.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default()
}
