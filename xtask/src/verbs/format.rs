//! Formatting automation for `fmt`, `fmt-check`, and `fix`.
//!
//! Stable rustfmt always runs. `.rustfmt-nightly` selects a toolchain, and `.rumdl.toml` enables
//! Markdown formatting through rumdl.

use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::{
    process::{self, Status},
    task,
    verb::Verb,
};

mod markdown;
mod rust;

/// Selects formatting or drift checking.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum FormatMode {
    Write,
    Check,
}

impl FormatMode {
    fn check_argument(self) -> Option<&'static str> {
        (self == Self::Check).then_some("--check")
    }
}

/// Optional rustfmt nightly-toolchain pin.
const NIGHTLY_FILE: &str = ".rustfmt-nightly";

/// Marker that enables Markdown formatting.
const RUMDL_FILE: &str = ".rumdl.toml";

/// Applies every active formatter.
pub(crate) fn run() -> Result<()> {
    task::run_all(&write_steps()?)?;
    process::result(Verb::FORMAT, Status::Done);
    Ok(())
}

/// Checks every formatter, aggregating failures without writing.
pub(crate) fn check() -> Result<()> {
    task::check_all(&check_steps()?, "run `just fmt`")?;
    process::result(Verb::FORMAT_CHECK, Status::Pass);
    Ok(())
}

/// Builds a format plan from repository marker files.
fn format_steps(mode: FormatMode) -> Result<Vec<task::Step>> {
    let mut steps = rust::format_steps(nightly_pin()?.as_deref(), mode);
    if Path::new(RUMDL_FILE).is_file() {
        steps.extend(markdown::format_step(&markdown_files()?, mode));
    }
    Ok(steps)
}

pub(super) fn write_steps() -> Result<Vec<task::Step>> {
    format_steps(FormatMode::Write)
}

pub(super) fn check_steps() -> Result<Vec<task::Step>> {
    format_steps(FormatMode::Check)
}

/// Reads the pinned toolchain when present.
fn nightly_pin() -> Result<Option<String>> {
    if !Path::new(NIGHTLY_FILE).is_file() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(NIGHTLY_FILE)
        .with_context(|| format!("reading {NIGHTLY_FILE} (run from the repo root)"))?;
    parse_nightly_pin(&raw).map(Some)
}

/// Parses a non-empty [`NIGHTLY_FILE`] toolchain name.
fn parse_nightly_pin(raw: &str) -> Result<String> {
    let pin = raw.trim();
    if pin.is_empty() {
        bail!(
            "{NIGHTLY_FILE} is empty; write a toolchain name (e.g. nightly-2025-11-01) or remove the file"
        );
    }
    Ok(pin.to_string())
}

/// Lists tracked and unignored Markdown files from NUL-delimited Git output.
fn markdown_files() -> Result<Vec<String>> {
    let output = process::capture(
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
    Ok(output
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

    #[test]
    fn check_steps_only_contain_formatters() {
        let steps = check_steps().unwrap();
        assert!(steps.iter().all(|step| step.label() != "clippy"));
    }
}
