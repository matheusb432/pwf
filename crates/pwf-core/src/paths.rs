//! Project path construction + prefix lookup over a resolved [`Config`].

use std::path::{Path, PathBuf};

use crate::config::Config;

/// `<notes_dir>/<name>` — the project's notes folder.
pub fn project_dir(notes_dir: &str, name: &str) -> PathBuf {
    Path::new(notes_dir).join(name)
}

/// `<notes_dir>/<name>/<name>.md` — the project's index file.
pub fn project_index_path(notes_dir: &str, name: &str) -> PathBuf {
    project_dir(notes_dir, name).join(format!("{name}.md"))
}

/// The configured id prefix for `name`, or `None` when unmapped or blank.
pub fn project_key<'a>(cfg: &'a Config, name: &str) -> Option<&'a str> {
    cfg.prefixes
        .get(name)
        .map(String::as_str)
        .filter(|key| !key.trim().is_empty())
}
