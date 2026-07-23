//! Captured child execution with optional live output.

use std::{
    io::{BufRead, BufReader, Read},
    process::{Command, Stdio},
    sync::Mutex,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};

use crate::task::Step;

pub(super) struct Execution {
    pub(super) passed: bool,
    pub(super) output: String,
    pub(super) elapsed: Duration,
}

pub(super) fn run_step(step: &Step, verbose: bool) -> Result<Execution> {
    let start = Instant::now();
    let mut child = Command::new(step.program())
        .args(step.arguments())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("spawning {}", step.label()))?;

    let stdout = child.stdout.take().expect("stdout is piped");
    let stderr = child.stderr.take().expect("stderr is piped");
    let output = Mutex::new(String::new());
    let read = std::thread::scope(|scope| {
        let stderr_reader = scope.spawn(|| collect(stderr, &output, verbose));
        let stdout_read = collect(stdout, &output, verbose);
        let stderr_read = stderr_reader.join().expect("stderr reader panicked");
        stdout_read.and(stderr_read)
    });
    read.with_context(|| format!("reading {} output", step.label()))?;

    let status = child
        .wait()
        .with_context(|| format!("waiting for {}", step.label()))?;
    Ok(Execution {
        passed: status.success(),
        output: output.into_inner().expect("output lock poisoned"),
        elapsed: start.elapsed(),
    })
}

fn collect(stream: impl Read, output: &Mutex<String>, verbose: bool) -> std::io::Result<()> {
    for line in BufReader::new(stream).lines() {
        let line = line?;
        if verbose {
            eprintln!("{line}");
        }
        let mut output = output.lock().expect("output lock poisoned");
        output.push_str(&line);
        output.push('\n');
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_stdout_of_a_passing_command() {
        let step = Step::new("version", "cargo", ["--version"]);
        let execution = run_step(&step, false).unwrap();
        assert!(execution.passed);
        assert!(execution.output.contains("cargo"));
    }

    #[test]
    fn captures_stderr_of_a_failing_command() {
        let step = Step::new("bogus", "cargo", ["definitely-not-a-subcommand"]);
        let execution = run_step(&step, false).unwrap();
        assert!(!execution.passed);
        assert!(execution.output.contains("error"));
    }
}
