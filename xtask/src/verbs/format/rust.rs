//! rustfmt and clippy command plans.

use super::FormatMode;
use crate::task::Step;

/// Plans workspace rustfmt with an optional pinned nightly.
///
/// Nightly writes run twice so comment wrapping converges; checks run once.
pub(super) fn format_steps(toolchain: Option<&str>, mode: FormatMode) -> Vec<Step> {
    let passes = match (toolchain, mode) {
        (Some(_), FormatMode::Write) => 2,
        _ => 1,
    };
    (0..passes)
        .map(|_| {
            Step::new("rustfmt", "cargo", toolchain.map(|t| format!("+{t}")))
                .with_arguments(["fmt", "--all"])
                .with_arguments(mode.check_argument())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args_of(step: &Step) -> Vec<&str> {
        step.arguments().iter().map(String::as_str).collect()
    }

    #[test]
    fn stable_fmt_is_a_single_plain_pass() {
        let write = format_steps(None, FormatMode::Write);
        assert_eq!(write.len(), 1);
        assert_eq!(args_of(&write[0]), ["fmt", "--all"]);

        let check = format_steps(None, FormatMode::Check);
        assert_eq!(check.len(), 1);
        assert_eq!(args_of(&check[0]), ["fmt", "--all", "--check"]);
    }

    #[test]
    fn pinned_nightly_prefixes_the_toolchain() {
        let steps = format_steps(Some("nightly-x"), FormatMode::Check);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].program(), "cargo");
        assert_eq!(
            args_of(&steps[0]),
            ["+nightly-x", "fmt", "--all", "--check"]
        );
    }

    #[test]
    fn nightly_write_double_passes_without_check() {
        let steps = format_steps(Some("nightly-x"), FormatMode::Write);
        assert_eq!(steps.len(), 2);
        assert!(steps.iter().all(|s| !args_of(s).contains(&"--check")));
    }
}
