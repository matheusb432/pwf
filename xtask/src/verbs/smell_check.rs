//! `smell-check-errors` — read-only production error-handling smell scan.
//!
//! Migrates `just pwf smell-check-errors`. The scan itself is still the bespoke
//! `scripts/smell-check-errors.sh` (rg queries + a hardcoded boundary allowlist + an awk
//! function-scope check); this verb shells it verbatim. Porting the allowlist to native Rust
//! is a deferred follow-up — behavior here is byte-identical to the retired recipe.

use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::{
    paths,
    proc::{self, Status},
};

/// Runs the error-smell script from the repo root; non-zero exit on smells (propagated).
pub(crate) fn run() -> Result<()> {
    if which_rg().is_err() {
        bail!("ripgrep (rg) is required for error smell checks");
    }
    let script = paths::repo_root()
        .join("scripts")
        .join("smell-check-errors.sh");
    let script = script.to_str().context("script path is not valid UTF-8")?;

    let status = Command::new("bash")
        .arg(script)
        .status()
        .context("spawning smell-check-errors")?;

    if status.success() {
        proc::result("smell-check-errors", Status::Pass);
        Ok(())
    } else {
        std::process::exit(status.code().unwrap_or(1));
    }
}

/// Succeeds when `rg` is resolvable on PATH.
fn which_rg() -> Result<()> {
    proc::capture("rg --version", "rg", &["--version"]).map(|_| ())
}
