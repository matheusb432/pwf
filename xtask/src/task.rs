//! Command plans and execution.

use std::time::Duration;

use anyhow::{Result, bail};

use crate::process;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Step {
    label: String,
    program: String,
    arguments: Vec<String>,
    deadline: Option<Duration>,
}

impl Step {
    pub(crate) fn new(
        label: impl Into<String>,
        program: impl Into<String>,
        arguments: impl IntoIterator<Item: Into<String>>,
    ) -> Self {
        Self {
            label: label.into(),
            program: program.into(),
            arguments: arguments.into_iter().map(Into::into).collect(),
            deadline: None,
        }
    }

    pub(crate) fn with_arguments(
        mut self,
        arguments_extra: impl IntoIterator<Item: Into<String>>,
    ) -> Self {
        self.arguments
            .extend(arguments_extra.into_iter().map(Into::into));
        self
    }

    pub(crate) fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = Some(deadline);
        self
    }

    pub(crate) fn label(&self) -> &str {
        &self.label
    }

    pub(crate) fn program(&self) -> &str {
        &self.program
    }

    pub(crate) fn arguments(&self) -> &[String] {
        &self.arguments
    }

    pub(crate) fn deadline(&self) -> Option<Duration> {
        self.deadline
    }
}

pub(crate) fn run_all(steps: &[Step]) -> Result<()> {
    for step in steps {
        process::run_step(step)?;
    }
    Ok(())
}

pub(crate) fn check_all(steps: &[Step], remediation: &str) -> Result<()> {
    let outcomes = steps
        .iter()
        .map(|step| process::step_succeeds(step).map(|success| (step.label(), success)))
        .collect::<Result<Vec<_>>>()?;
    if let Some(message) = failure_message(&outcomes, remediation) {
        bail!(message);
    }
    Ok(())
}

fn failure_message(outcomes: &[(&str, bool)], remediation: &str) -> Option<String> {
    let failed = outcomes
        .iter()
        .filter_map(|(label, success)| (!*success).then_some(*label))
        .collect::<Vec<_>>();
    if failed.is_empty() {
        None
    } else {
        Some(format!(
            "check failed in: {} - {remediation}",
            failed.join(", ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_message_lists_failed_steps() {
        let outcomes = [("format", false), ("docs", true), ("lint", false)];
        assert_eq!(
            failure_message(&outcomes, "repair it").as_deref(),
            Some("check failed in: format, lint - repair it")
        );
        assert_eq!(failure_message(&[("format", true)], "repair it"), None);
    }
}
