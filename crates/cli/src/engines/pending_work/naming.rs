use std::path::Path;

pub use pwf_core::{
    date::stamp_date,
    paths::{project_dir, project_index_path, project_key},
};

/// Returns a forward-slash relative path, or the target file name when it is outside `base`.
pub(super) fn pathdiff_forward(base: &str, target: &Path) -> String {
    let base_path = Path::new(base);
    if let Ok(rel) = target.strip_prefix(base_path) {
        return rel.to_string_lossy().replace('\\', "/");
    }
    target
        .file_name()
        .map(|n| n.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default()
}
