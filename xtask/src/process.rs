//! Process execution and result output.

use std::{
    ffi::OsStr,
    process::{Command, ExitStatus},
};

use anyhow::{Context, Result, bail};

use crate::{task::Step, verb::Verb};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    Pass,
    Done,
}

impl std::fmt::Display for Status {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Pass => "PASS",
            Self::Done => "DONE",
        })
    }
}

pub(crate) fn run_step(step: &Step) -> Result<()> {
    let status = exit_status(step.label(), step.program(), step.arguments())?;
    ensure_success(step.label(), status)
}

pub(crate) fn step_succeeds(step: &Step) -> Result<bool> {
    Ok(exit_status(step.label(), step.program(), step.arguments())?.success())
}

pub(crate) fn run(label: &str, program: &str, arguments: &[&str]) -> Result<()> {
    let status = exit_status(label, program, arguments)?;
    ensure_success(label, status)
}

fn exit_status<I, S>(label: &str, program: &str, arguments: I) -> Result<ExitStatus>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Command::new(program)
        .args(arguments)
        .status()
        .with_context(|| format!("spawning {label}"))
}

fn ensure_success(label: &str, status: ExitStatus) -> Result<()> {
    if !status.success() {
        bail!("{label} failed (exit {})", status.code().unwrap_or(-1));
    }
    Ok(())
}

pub(crate) fn capture(label: &str, program: &str, arguments: &[&str]) -> Result<String> {
    let output = Command::new(program)
        .args(arguments)
        .output()
        .with_context(|| format!("spawning {label}"))?;
    if !output.status.success() {
        bail!(
            "{label} failed (exit {})",
            output.status.code().unwrap_or(-1)
        );
    }
    String::from_utf8(output.stdout).context("non-UTF-8 output")
}

pub(crate) fn result(scope: Verb, status: Status) {
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
        let error = run("boom", "false", &[]).unwrap_err();
        assert!(error.to_string().contains("boom failed"));
    }

    #[test]
    fn run_contextualizes_a_missing_program() {
        let error = run("rumdl", "definitely-not-a-real-binary-xyz", &[]).unwrap_err();
        assert!(error.to_string().contains("spawning rumdl"));
    }

    #[test]
    fn step_exit_is_available_as_a_result_or_boolean() {
        let passing = Step::new("passing", "true", Vec::<String>::new());
        let failing = Step::new("failing", "false", Vec::<String>::new());
        assert!(step_succeeds(&passing).unwrap());
        assert!(!step_succeeds(&failing).unwrap());
        assert!(run_step(&failing).is_err());
    }
}
