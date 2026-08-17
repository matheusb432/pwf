use std::{
    process::{Command, ExitStatus},
    time::Duration,
};

use anyhow::{Context, Result, bail};

use crate::child_process;

pub(crate) fn run(label: &str, command: &mut Command) -> Result<()> {
    let status = command
        .status()
        .with_context(|| format!("spawning {label}"))?;
    ensure_success(label, status)
}

pub(crate) fn run_bounded(label: &str, command: Command, deadline: Duration) -> Result<()> {
    let status = child_process::run(command, label, deadline)?;
    ensure_success(label, status)
}

fn ensure_success(label: &str, status: ExitStatus) -> Result<()> {
    if !status.success() {
        bail!("{label} failed (exit {})", status.code().unwrap_or(-1));
    }
    Ok(())
}
