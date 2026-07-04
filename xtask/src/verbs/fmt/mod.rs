//! The `fmt`, `fmt-check`, and `fix` verbs — formatting + lint automation.
//!
//! Formatters are gated on repo-root marker files so a fresh repo works with zero extra setup:
//! stable `cargo fmt` always runs; a `.rustfmt-nightly` pin (one line, e.g. `nightly-2025-11-01`)
//! switches rustfmt to that toolchain; a `.mdformat.toml` opts Markdown into mdformat (via `uvx`).
//! Adapters build command plans; the shared task runner executes them.

use std::path::Path;

use anyhow::{Context, Result, bail};
use clap::Args;

use crate::{
    proc::{self, ProcessRunner, Status},
    task,
};

mod markdown;
mod rust;

/// Extra args for the `fix` verb, forwarded verbatim to `cargo clippy --fix`.
#[derive(Args)]
pub(crate) struct FixArgs {
    /// e.g. `-- -W clippy::pedantic`
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub(crate) args: Vec<String>,
}

/// Whether a formatter run rewrites files or only reports drift.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum FmtMode {
    Write,
    Check,
}

impl FmtMode {
    /// The `--check` flag when verifying, `None` when writing.
    fn check_flag(self) -> Option<&'static str> {
        (self == Self::Check).then_some("--check")
    }
}

/// Optional pin switching rustfmt to a nightly toolchain (enables nightly-only rustfmt.toml keys).
const NIGHTLY_FILE: &str = ".rustfmt-nightly";

/// Optional marker opting the repo into Markdown formatting.
const MDFORMAT_FILE: &str = ".mdformat.toml";

/// Applies every active formatter in place.
pub(crate) fn fmt() -> Result<()> {
    task::run_all(&ProcessRunner, &format_steps(FmtMode::Write)?)?;
    proc::result("fmt", Status::Done);
    Ok(())
}

/// Verifies formatting and lints without writing. Every step runs even after one fails,
/// so a single invocation reports all offenders.
pub(crate) fn fmt_check() -> Result<()> {
    let mut steps = format_steps(FmtMode::Check)?;
    steps.push(rust::clippy_check_step());
    task::check_all(&ProcessRunner, &steps, "run `just fix` / `just fmt`")?;
    proc::result("fmt-check", Status::Pass);
    Ok(())
}

/// Applies clippy's machine-applicable fixes, then reformats whatever clippy rewrote.
/// `extra` is forwarded verbatim to clippy (e.g. `-- -W clippy::nursery`).
pub(crate) fn fix(extra: &[String]) -> Result<()> {
    task::run_all(&ProcessRunner, &[rust::clippy_fix_step(extra)])?;
    task::run_all(&ProcessRunner, &format_steps(FmtMode::Write)?)?;
    proc::result("fix", Status::Done);
    Ok(())
}

/// Gathers the repo's formatter markers and builds the step plan for `mode`.
fn format_steps(mode: FmtMode) -> Result<Vec<task::Step>> {
    let mut steps = rust::format_steps(nightly_pin()?.as_deref(), mode);
    if Path::new(MDFORMAT_FILE).is_file() {
        steps.extend(markdown::format_step(&markdown_files()?, mode));
    }
    Ok(steps)
}

/// The pinned toolchain from [`NIGHTLY_FILE`], or `None` when the repo doesn't pin one.
fn nightly_pin() -> Result<Option<String>> {
    if !Path::new(NIGHTLY_FILE).is_file() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(NIGHTLY_FILE)
        .with_context(|| format!("reading {NIGHTLY_FILE} (run from the repo root)"))?;
    parse_nightly_pin(&raw).map(Some)
}

/// Parses a present [`NIGHTLY_FILE`] marker's contents into a toolchain name.
/// A blank marker is a misconfiguration — erroring beats spawning a broken `cargo +` (no
/// toolchain).
fn parse_nightly_pin(raw: &str) -> Result<String> {
    let pin = raw.trim();
    if pin.is_empty() {
        bail!(
            "{NIGHTLY_FILE} is empty; write a toolchain name (e.g. nightly-2025-11-01) or remove the file"
        );
    }
    Ok(pin.to_string())
}

/// Tracked + untracked-but-not-ignored Markdown files (NUL-split `git ls-files`), so
/// gitignored files are never formatted.
fn markdown_files() -> Result<Vec<String>> {
    let out = proc::capture(
        "git ls-files",
        "git",
        &[
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "-z",
            "--",
            "*.md",
        ],
    )?;
    Ok(out
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nightly_pin_trims_the_toolchain_name() {
        assert_eq!(
            parse_nightly_pin("  nightly-2025-11-01\n").unwrap(),
            "nightly-2025-11-01"
        );
    }

    #[test]
    fn blank_nightly_marker_is_rejected() {
        assert!(parse_nightly_pin("").is_err());
        assert!(parse_nightly_pin("   \n\t").is_err());
    }
}
