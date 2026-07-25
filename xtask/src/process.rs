//! Process execution and result output.

use std::{
    ffi::OsStr,
    process::{Command, ExitStatus},
};

use anyhow::{Context, Result, bail};

use crate::{child_process, task::Step, verb::Verb};

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
    let status = step_exit_status(step)?;
    ensure_success(step.label(), status)
}

pub(crate) fn step_succeeds(step: &Step) -> Result<bool> {
    Ok(step_exit_status(step)?.success())
}

pub(crate) fn run(label: &str, program: &str, arguments: &[&str]) -> Result<()> {
    let status = exit_status(label, program, arguments)?;
    ensure_success(label, status)
}

fn step_exit_status(step: &Step) -> Result<ExitStatus> {
    let Some(deadline) = step.deadline() else {
        return exit_status(step.label(), step.program(), step.arguments());
    };

    let mut command = Command::new(step.program());
    command.args(step.arguments());
    child_process::run(command, step.label(), deadline)
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

    #[test]
    fn deadline_step_routes_through_bounded_process_tree_runner() {
        let executable = std::env::current_exe().expect("current test executable");
        let step = Step::new(
            "sleeping step",
            executable.to_string_lossy(),
            ["--exact", "process::tests::sleeping_step_stub", "--ignored"],
        )
        .with_deadline(std::time::Duration::from_millis(100));

        let error = step_succeeds(&step).expect_err("sleeping step must time out");

        assert!(crate::child_process::is_timeout(&error));
    }

    #[test]
    #[ignore = "process deadline fixture"]
    fn sleeping_step_stub() {
        std::thread::sleep(std::time::Duration::from_secs(10));
    }
}
