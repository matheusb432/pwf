//! Project path construction and prefix lookup over a resolved [`Config`].

use std::path::{Path, PathBuf};

use crate::config::Config;

/// Returns the project's `<notes_dir>/<name>` directory.
pub fn project_dir(notes_dir: &str, name: &str) -> PathBuf {
    Path::new(notes_dir).join(name)
}

/// Returns the project's `<notes_dir>/<name>/<name>.md` index path.
pub fn project_index_path(notes_dir: &str, name: &str) -> PathBuf {
    project_dir(notes_dir, name).join(format!("{name}.md"))
}

/// Returns the configured id prefix, excluding missing and blank values.
pub fn project_key<'a>(cfg: &'a Config, name: &str) -> Option<&'a str> {
    cfg.prefixes
        .get(name)
        .map(String::as_str)
        .filter(|key| !key.trim().is_empty())
}
