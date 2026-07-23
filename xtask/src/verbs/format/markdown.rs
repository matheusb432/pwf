//! rumdl command planning for repositories with `.rumdl.toml`.

use super::FormatMode;
use crate::task::Step;

/// Builds a rumdl step, or returns [`None`] when there are no Markdown files.
pub(super) fn format_step(files: &[String], mode: FormatMode) -> Option<Step> {
    if files.is_empty() {
        return None;
    }
    Some(
        Step::new("rumdl", "rumdl", ["fmt"])
            .with_arguments(mode.check_argument())
            .with_arguments(files.iter().cloned()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_step_carries_files_and_check_flag() {
        let files = vec!["a.md".to_string(), "docs/b.md".to_string()];
        let write = format_step(&files, FormatMode::Write).unwrap();
        assert_eq!(write.program(), "rumdl");
        assert_eq!(write.arguments(), ["fmt", "a.md", "docs/b.md"]);

        let check = format_step(&files, FormatMode::Check).unwrap();
        assert_eq!(check.arguments(), ["fmt", "--check", "a.md", "docs/b.md"]);
    }

    #[test]
    fn markdown_step_is_none_without_files() {
        assert!(format_step(&[], FormatMode::Write).is_none());
    }
}
