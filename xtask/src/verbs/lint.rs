//! Lint verb and Clippy plans.

use anyhow::Result;

use crate::{
    process::{self, Status},
    task::{self, Step},
    verb::Verb,
};

pub(crate) fn run() -> Result<()> {
    task::check_all(&[check_step()], "run `just fix`")?;
    process::result(Verb::LINT, Status::Pass);
    Ok(())
}

pub(super) fn check_step() -> Step {
    Step::new(
        "clippy",
        "cargo",
        [
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
    )
}

pub(super) fn fix_step(arguments_extra: &[String]) -> Step {
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
    .with_arguments(arguments_extra.iter().cloned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_step_targets_the_workspace_with_denied_warnings() {
        let step = check_step();
        assert_eq!(step.program(), "cargo");
        assert_eq!(
            step.arguments(),
            [
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings"
            ]
        );
    }

    #[test]
    fn fix_step_forwards_extra_arguments() {
        let step = fix_step(&["-W".into(), "clippy::nursery".into()]);
        assert!(step.arguments().contains(&"--fix".to_string()));
        assert!(step.arguments().contains(&"--allow-dirty".to_string()));
        assert!(
            step.arguments()
                .ends_with(&["-W".to_string(), "clippy::nursery".to_string()])
        );
    }
}
