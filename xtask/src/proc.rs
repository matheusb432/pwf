//! Child-process execution and stable `RESULT scope=… status=…` reporting.

use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::task::{Step, StepRunner};

/// Selects the `status=` value in a terminal `RESULT` line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    Pass,
    Done,
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Pass => "PASS",
            Self::Done => "DONE",
        })
    }
}

/// Runs steps as child processes.
pub(crate) struct ProcessRunner;

impl StepRunner for ProcessRunner {
    fn run(&self, step: &Step) -> Result<()> {
        let status = Command::new(step.program())
            .args(step.argv())
            .status()
            .with_context(|| format!("spawning {}", step.label()))?;
        if !status.success() {
            bail!(
                "{} failed (exit {})",
                step.label(),
                status.code().unwrap_or(-1)
            );
        }
        Ok(())
    }

    fn succeeds(&self, step: &Step) -> Result<bool> {
        let status = Command::new(step.program())
            .args(step.argv())
            .status()
            .with_context(|| format!("spawning {}", step.label()))?;
        Ok(status.success())
    }
}

/// Runs a child process and tags non-zero exits with `label`.
pub(crate) fn run(label: &str, program: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(program)
        .args(args)
        .status()
        .with_context(|| format!("spawning {label}"))?;
    if !status.success() {
        bail!("{label} failed (exit {})", status.code().unwrap_or(-1));
    }
    Ok(())
}

/// Runs a child process and captures UTF-8 stdout.
pub(crate) fn capture(label: &str, program: &str, args: &[&str]) -> Result<String> {
    let out = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("spawning {label}"))?;
    if !out.status.success() {
        bail!("{label} failed (exit {})", out.status.code().unwrap_or(-1));
    }
    String::from_utf8(out.stdout).context("non-UTF-8 output")
}

/// Emits the byte-stable `RESULT` line consumed by automation.
pub(crate) fn result(scope: &str, status: Status) {
    println!("RESULT scope={scope} status={status}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_ok_on_true() {
        assert!(run("noop", "true", &[]).is_ok());
    }

    #[test]
    fn run_err_on_false() {
        let err = run("boom", "false", &[]).unwrap_err();
        assert!(err.to_string().contains("boom failed"));
    }

    #[test]
    fn run_contextualizes_a_missing_program() {
        let err = run("mdformat", "definitely-not-a-real-binary-xyz", &[]).unwrap_err();
        assert!(err.to_string().contains("spawning mdformat"));
    }

    #[test]
    fn runner_reports_exit_as_data_for_checks() {
        let ok = Step::new("ok", "true", Vec::<String>::new());
        let bad = Step::new("bad", "false", Vec::<String>::new());
        assert!(ProcessRunner.succeeds(&ok).unwrap());
        assert!(!ProcessRunner.succeeds(&bad).unwrap());
        assert!(ProcessRunner.run(&bad).is_err());
    }
}
