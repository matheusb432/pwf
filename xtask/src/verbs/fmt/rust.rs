//! rustfmt and clippy command plans.

use super::FmtMode;
use crate::task::Step;

/// Plans workspace rustfmt with an optional pinned nightly.
///
/// Nightly writes run twice so comment wrapping converges; checks run once.
pub(super) fn format_steps(toolchain: Option<&str>, mode: FmtMode) -> Vec<Step> {
    let passes = match (toolchain, mode) {
        (Some(_), FmtMode::Write) => 2,
        _ => 1,
    };
    (0..passes)
        .map(|_| {
            Step::new("rustfmt", "cargo", toolchain.map(|t| format!("+{t}")))
                .args(["fmt", "--all"])
                .args(mode.check_flag())
        })
        .collect()
}

/// Plans whole-workspace Clippy with warnings denied.
pub(super) fn clippy_check_step() -> Step {
    Step::new(
        "clippy",
        "cargo",
        ["clippy", "--workspace", "--all-targets"],
    )
    .args(["--", "-D", "warnings"])
}

/// Plans whole-workspace machine-applicable Clippy fixes with extra arguments.
pub(super) fn clippy_fix_step(extra: &[String]) -> Step {
    Step::new(
        "clippy:fix",
        "cargo",
        [
            "clippy",
            "--fix",
            "--workspace",
            "--all-targets",
            "--allow-dirty",
        ],
    )
    .args(extra.iter().cloned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args_of(step: &Step) -> Vec<&str> {
        step.argv().iter().map(String::as_str).collect()
    }

    #[test]
    fn stable_fmt_is_a_single_plain_pass() {
        let write = format_steps(None, FmtMode::Write);
        assert_eq!(write.len(), 1);
        assert_eq!(args_of(&write[0]), ["fmt", "--all"]);

        let check = format_steps(None, FmtMode::Check);
        assert_eq!(check.len(), 1);
        assert_eq!(args_of(&check[0]), ["fmt", "--all", "--check"]);
    }

    #[test]
    fn pinned_nightly_prefixes_the_toolchain() {
        let steps = format_steps(Some("nightly-x"), FmtMode::Check);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].program(), "cargo");
        assert_eq!(
            args_of(&steps[0]),
            ["+nightly-x", "fmt", "--all", "--check"]
        );
    }

    #[test]
    fn nightly_write_double_passes_without_check() {
        let steps = format_steps(Some("nightly-x"), FmtMode::Write);
        assert_eq!(steps.len(), 2);
        assert!(steps.iter().all(|s| !args_of(s).contains(&"--check")));
    }

    #[test]
    fn clippy_steps_target_the_whole_workspace() {
        let check_step = clippy_check_step();
        let check = args_of(&check_step);
        assert!(check.contains(&"clippy") && check.contains(&"--workspace"));
        assert!(!check.contains(&"--fix"));
        assert!(check.ends_with(&["--", "-D", "warnings"]));

        let fix = clippy_fix_step(&["-W".into(), "clippy::nursery".into()]);
        let a = args_of(&fix);
        assert!(a.contains(&"--fix") && a.contains(&"--allow-dirty"));
        assert!(a.ends_with(&["-W", "clippy::nursery"]));
    }
}
