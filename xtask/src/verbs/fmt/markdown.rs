//! Markdown (mdformat) command plan — active only when the repo opts in via `.mdformat.toml`.

use super::FmtMode;
use crate::task::Step;

/// Plugins `uvx` provisions alongside mdformat; keep in lockstep with `.mdformat.toml`.
const PLUGINS: &[&str] = &[
    "mdformat-gfm",
    "mdformat-gfm-alerts",
    "mdformat-wikilink",
    "mdformat-frontmatter",
];

/// The mdformat step for `mode`, or `None` when there are no Markdown files to format.
pub(super) fn format_step(files: &[String], mode: FmtMode) -> Option<Step> {
    if files.is_empty() {
        return None;
    }
    Some(
        Step::new("mdformat", "uvx", ["--python", "3.13"])
            .args(PLUGINS.iter().flat_map(|p| ["--with", *p]))
            .args(["mdformat"])
            .args(mode.check_flag())
            .args(files.iter().cloned()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_step_carries_plugins_files_and_check_flag() {
        let files = vec!["a.md".to_string(), "docs/b.md".to_string()];
        let write = format_step(&files, FmtMode::Write).unwrap();
        assert_eq!(write.program(), "uvx");
        for plugin in PLUGINS {
            assert!(write.argv().iter().any(|a| a == plugin));
        }
        assert!(!write.argv().iter().any(|a| a == "--check"));
        assert!(write.argv().iter().any(|a| a == "docs/b.md"));

        let check = format_step(&files, FmtMode::Check).unwrap();
        assert!(check.argv().iter().any(|a| a == "--check"));
    }

    #[test]
    fn markdown_step_is_none_without_files() {
        assert!(format_step(&[], FmtMode::Write).is_none());
    }
}
