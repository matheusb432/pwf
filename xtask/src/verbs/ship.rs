//! Release workflow.

use anyhow::Result;

use crate::{
    process::{self, Status},
    task::{self, Step},
    verb::Verb,
};

pub(crate) fn run() -> Result<()> {
    task::run_all(&steps())?;
    process::result(Verb::SHIP, Status::Pass);
    Ok(())
}

fn steps() -> [Step; 2] {
    [
        Step::new("test:all", "just", ["test", "--all"]),
        Step::new("build:release", "cargo", ["build", "--release"]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_run_the_full_test_gate_before_the_release_build() {
        let steps = steps();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].program(), "just");
        assert_eq!(steps[0].arguments(), ["test", "--all"]);
        assert_eq!(steps[1].program(), "cargo");
        assert_eq!(steps[1].arguments(), ["build", "--release"]);
    }
}
