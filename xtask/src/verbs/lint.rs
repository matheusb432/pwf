//! Lint verb and Clippy plans.

use anyhow::Result;

use crate::{
    process::{self, Status},
    task::{self, Step},
    verb::Verb,
};

pub(crate) fn run() -> Result<()> {
    task::check_all(&check_steps(), "run `just fix`")?;
    process::result(Verb::LINT, Status::Pass);
    Ok(())
}

pub(super) fn check_steps() -> [Step; 2] {
    [
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
        ),
        Step::new(
            "clippy:e2e",
            "cargo",
            [
                "clippy", "-p", "pwf-e2e", "--test", "e2e", "--", "-D", "warnings",
            ],
        ),
    ]
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
    fn check_steps_cover_the_workspace_and_hidden_e2e_target() {
        let steps = check_steps();

        assert_eq!(steps[0].program(), "cargo");
        assert_eq!(
            steps[0].arguments(),
            [
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings"
            ]
        );
        assert_eq!(steps[1].program(), "cargo");
        assert_eq!(
            steps[1].arguments(),
            [
                "clippy", "-p", "pwf-e2e", "--test", "e2e", "--", "-D", "warnings"
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
