//! mdformat command planning for repositories with `.mdformat.toml`.

use super::FormatMode;
use crate::task::Step;

/// Plugins provisioned with mdformat and kept aligned with `.mdformat.toml`.
const PLUGINS: &[&str] = &[
    "mdformat-gfm",
    "mdformat-gfm-alerts",
    "mdformat-wikilink",
    "mdformat-frontmatter",
];

/// Builds an mdformat step, or returns [`None`] when there are no Markdown files.
pub(super) fn format_step(files: &[String], mode: FormatMode) -> Option<Step> {
    if files.is_empty() {
        return None;
    }
    Some(
        Step::new("mdformat", "uvx", ["--python", "3.13"])
            .with_arguments(PLUGINS.iter().flat_map(|plugin| ["--with", *plugin]))
            .with_arguments(["mdformat"])
            .with_arguments(mode.check_argument())
            .with_arguments(files.iter().cloned()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_step_carries_plugins_files_and_check_flag() {
        let files = vec!["a.md".to_string(), "docs/b.md".to_string()];
        let write = format_step(&files, FormatMode::Write).unwrap();
        assert_eq!(write.program(), "uvx");
        for plugin in PLUGINS {
            assert!(write.arguments().iter().any(|argument| argument == plugin));
        }
        assert!(
            !write
                .arguments()
                .iter()
                .any(|argument| argument == "--check")
        );
        assert!(
            write
                .arguments()
                .iter()
                .any(|argument| argument == "docs/b.md")
        );

        let check = format_step(&files, FormatMode::Check).unwrap();
        assert!(
            check
                .arguments()
                .iter()
                .any(|argument| argument == "--check")
        );
    }

    #[test]
    fn markdown_step_is_none_without_files() {
        assert!(format_step(&[], FormatMode::Write).is_none());
    }
}
