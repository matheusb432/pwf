use std::{path::Path, sync::LazyLock};

use regex::Regex;

static DASH_UNDERSCORE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[-_]+").unwrap());
static DATE_SLUG_PREFIX_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\d{4}-\d{2}-\d{2}-").unwrap());

/// Converts hyphens and underscores in a project name to lowercase spaces.
pub fn project_title_prefix(name: &str) -> String {
    DASH_UNDERSCORE_RE
        .replace_all(name, " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Derives a `continue` title from a dated handoff path.
pub fn handoff_title_from_path(path: &str) -> String {
    let stem = Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let slug = DATE_SLUG_PREFIX_RE.replace(stem, "");
    let words: Vec<&str> = DASH_UNDERSCORE_RE
        .split(&slug)
        .filter(|w| !w.is_empty())
        .collect();
    if words.is_empty() {
        "continue handoff".to_string()
    } else {
        format!("continue {}", words.join(" "))
    }
}

pub fn get_title_from_continue_path(project_name: &str, path: &str) -> String {
    let stem = Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let slug = DATE_SLUG_PREFIX_RE.replace(stem, "");
    let excluded = ["kickoff", "handoff", "plan"];
    let words: Vec<String> = DASH_UNDERSCORE_RE
        .split(&slug)
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .filter(|w| !excluded.contains(&w.as_str()))
        .collect();
    if words.is_empty() {
        format!("{} plan", project_title_prefix(project_name))
    } else {
        format!("{} {}", project_title_prefix(project_name), words.join(" "))
    }
}

/// Returns the one-based line number at a byte offset.
pub fn line_number(text: &str, index: usize) -> usize {
    if index == 0 {
        return 1;
    }
    // Callers normalize line endings to LF.
    1 + text[..index].matches('\n').count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handoff_title_from_path_strips_date_prefix() {
        assert_eq!(
            handoff_title_from_path("docs/handoffs/2026-01-01-api-cleanup.md"),
            "continue api cleanup"
        );
    }
}
