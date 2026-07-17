//! Labeled command plans with ordered execution and failure aggregation.

use anyhow::{Result, bail};

/// Describes one labeled program invocation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Step {
    label: String,
    program: String,
    args: Vec<String>,
}

impl Step {
    pub(crate) fn new(
        label: impl Into<String>,
        program: impl Into<String>,
        args: impl IntoIterator<Item: Into<String>>,
    ) -> Self {
        Self {
            label: label.into(),
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
        }
    }

    /// Appends arguments from arrays, iterators, or conditional options.
    pub(crate) fn args(mut self, extra: impl IntoIterator<Item: Into<String>>) -> Self {
        self.args.extend(extra.into_iter().map(Into::into));
        self
    }

    pub(crate) fn label(&self) -> &str {
        &self.label
    }

    pub(crate) fn program(&self) -> &str {
        &self.program
    }

    pub(crate) fn argv(&self) -> &[String] {
        &self.args
    }
}

/// Executes planned command steps.
pub(crate) trait StepRunner {
    fn run(&self, step: &Step) -> Result<()>;

    /// Reports non-zero exits as data so check gates can aggregate failures.
    fn succeeds(&self, step: &Step) -> Result<bool>;
}

/// Runs steps in order and stops at the first failure.
pub(crate) fn run_all(runner: &impl StepRunner, steps: &[Step]) -> Result<()> {
    for step in steps {
        runner.run(step)?;
    }
    Ok(())
}

/// Runs every check and reports all failures together.
pub(crate) fn check_all(runner: &impl StepRunner, steps: &[Step], remediation: &str) -> Result<()> {
    let failed = failed_labels(runner, steps)?;
    if !failed.is_empty() {
        bail!("check failed in: {} — {remediation}", failed.join(", "));
    }
    Ok(())
}

fn failed_labels(runner: &impl StepRunner, steps: &[Step]) -> Result<Vec<String>> {
    let mut failed = Vec::new();
    for step in steps {
        if !runner.succeeds(step)? {
            failed.push(step.label.clone());
        }
    }
    Ok(failed)
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::HashSet};

    use super::*;

    #[derive(Default)]
    struct FakeRunner {
        failing: HashSet<String>,
        calls: RefCell<Vec<String>>,
    }

    impl FakeRunner {
        fn failing(labels: &[&str]) -> Self {
            Self {
                failing: labels.iter().map(|label| (*label).to_string()).collect(),
                calls: RefCell::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<String> {
            self.calls.borrow().clone()
        }
    }

    impl StepRunner for FakeRunner {
        fn run(&self, step: &Step) -> Result<()> {
            self.calls.borrow_mut().push(step.label().to_string());
            Ok(())
        }

        fn succeeds(&self, step: &Step) -> Result<bool> {
            self.calls.borrow_mut().push(step.label().to_string());
            Ok(!self.failing.contains(step.label()))
        }
    }

    fn step(label: &str) -> Step {
        Step::new(label, "tool", Vec::<String>::new())
    }

    #[test]
    fn step_builder_appends_arrays_options_and_iterators() {
        let step = Step::new("fmt", "cargo", ["fmt", "--all"])
            .args(Some("--check"))
            .args(None::<&str>)
            .args(["a.md".to_string()]);
        assert_eq!(step.program(), "cargo");
        assert_eq!(step.argv(), ["fmt", "--all", "--check", "a.md"]);
    }

    #[test]
    fn run_all_uses_the_runner_in_order() {
        let runner = FakeRunner::default();
        run_all(&runner, &[step("first"), step("second")]).unwrap();
        assert_eq!(runner.calls(), ["first", "second"]);
    }

    #[test]
    fn check_all_aggregates_all_failed_steps() {
        let runner = FakeRunner::failing(&["fmt", "lint"]);
        let err = check_all(
            &runner,
            &[step("fmt"), step("docs"), step("lint")],
            "repair it",
        )
        .unwrap_err();

        assert_eq!(runner.calls(), ["fmt", "docs", "lint"]);
        assert_eq!(err.to_string(), "check failed in: fmt, lint — repair it");
    }
}
