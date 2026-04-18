// Date, id allocation, and path/key helpers.

use crate::config::Config;
use chrono::Local;
use regex::Regex;
use std::path::{Path, PathBuf};

const ARCHIVE_DIR: &str = "_archive";

pub fn project_dir(notes_dir: &str, name: &str) -> PathBuf {
    Path::new(notes_dir).join(name)
}

/// Convert a Path to a forward-slash string for JSON output (portable across OSes).
pub(super) fn path_str(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

pub fn project_index_path(notes_dir: &str, name: &str) -> PathBuf {
    project_dir(notes_dir, name).join(format!("{name}.md"))
}

pub fn project_key<'a>(cfg: &'a Config, name: &str) -> Option<&'a str> {
    cfg.prefixes
        .get(name)
        .map(String::as_str)
        .filter(|key| !key.trim().is_empty())
}

pub fn stamp_date(date: &Option<String>) -> String {
    match date {
        Some(d) => d.clone(),
        None => Local::now().format("%Y-%m-%d").to_string(),
    }
}

/// Allocates `KEY-{max+1:04}` by scanning `<dir>/KEY-*.md` and archived notes.
/// Gaps are preserved.
pub fn next_work_item_id(dir: &Path, key: &str) -> String {
    let re = Regex::new(&format!(r"^{}-(\d{{4}})$", regex::escape(key))).unwrap();
    let mut max = 0;
    scan_max_work_item_number(dir, &re, &mut max);
    scan_max_work_item_number(&dir.join(ARCHIVE_DIR), &re, &mut max);
    format!("{key}-{:04}", max + 1)
}

fn scan_max_work_item_number(dir: &Path, re: &Regex, max: &mut i32) {
    if !dir.exists() {
        return;
    }

    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        if let Some(stem) = p.file_stem().and_then(|s| s.to_str())
            && let Some(c) = re.captures(stem)
        {
            let n: i32 = c[1].parse().unwrap_or(0);
            if n > *max {
                *max = n;
            }
        }
    }
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
