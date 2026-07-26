//! Captured gate runs and stable result output.

mod capture;
mod counts;
mod render;

use std::{fmt::Write as _, io::Write as _, path::PathBuf};

use anyhow::{Context, Result, bail};
pub(crate) use counts::Kind;

use crate::task::Step;

const FAILURE_TAIL_LINES: usize = 80;

pub(crate) struct Job {
    step: Step,
    kind: Kind,
}

impl Job {
    pub(crate) fn new(step: Step, kind: Kind) -> Self {
        Self { step, kind }
    }
}

pub(crate) fn run(scope: &str, jobs: &[Job], verbose: bool) -> Result<()> {
    let log = log_path(scope);
    if let Some(parent) = log.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }

    let mut log_content = String::new();
    let mut outcomes = Vec::with_capacity(jobs.len());
    let mut failures = Vec::new();
    for job in jobs {
        let execution = capture::run_step(&job.step, verbose)?;
        let _ = writeln!(log_content, "=== step: {} ===", job.step.label());
        log_content.push_str(&execution.output);
        if !execution.passed {
            failures.push((job.step.label().to_string(), execution.output.clone()));
        }
        outcomes.push(render::Outcome {
            label: job.step.label().to_string(),
            passed: execution.passed,
            summary: counts::summarize(job.kind, &execution.output),
            elapsed: execution.elapsed,
        });
    }

    std::fs::write(&log, &log_content).with_context(|| format!("writing {}", log.display()))?;

    print!(
        "{}",
        render::summary_table(scope, &outcomes, color_enabled())
    );
    if !verbose {
        surface_failures(&failures);
    }
    println!(
        "{}",
        render::result_line(scope, &outcomes, &log.display().to_string())
    );

    if failures.is_empty() {
        Ok(())
    } else {
        bail!("{scope} failed (full output: {})", log.display());
    }
}

fn surface_failures(failures: &[(String, String)]) {
    let mut stderr = std::io::stderr().lock();
    for (label, output) in failures {
        writeln!(stderr, "--- {label} failed (tail) ---").ok();
        let lines: Vec<&str> = output.lines().collect();
        let start = lines.len().saturating_sub(FAILURE_TAIL_LINES);
        for line in &lines[start..] {
            writeln!(stderr, "{line}").ok();
        }
    }
}

/// Resolves summary color without terminal detection; `NO_COLOR` wins and plain is the default.
fn color_enabled() -> bool {
    std::env::var_os("NO_COLOR").is_none() && std::env::var_os("CLICOLOR_FORCE").is_some()
}

fn log_path(scope: &str) -> PathBuf {
    let target =
        std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| PathBuf::from("target"), PathBuf::from);
    target
        .join("xtask")
        .join("logs")
        .join(format!("{scope}.log"))
}
