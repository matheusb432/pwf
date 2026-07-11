//! Command-line surface. clap derives `--help` from the doc comments here and on each verb's
//! `Args` struct — keep them the single source of truth for the verb documentation.

use clap::{Parser, Subcommand};

use crate::verbs::{fmt::FixArgs, install::UpdateArgs, test::TestArgs};

/// pwf's embedded dev/release automation (xtask). Never installed — run via `cargo run -p xtask`.
#[derive(Parser)]
#[command(version, about = "pwf's embedded dev/release automation (xtask)")]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

/// One arm per automation verb, each dispatched to its [`crate::verbs`] module. Let clap validate
/// the surface — `ValueEnum` for closed choices, `conflicts_with` for exclusive flags.
#[derive(Subcommand)]
pub(crate) enum Command {
    /// Format the repo in place (stable/pinned-nightly rustfmt; mdformat when `.mdformat.toml`
    /// present).
    Fmt,
    /// Verify formatting and lint without writing; exits non-zero on drift.
    FmtCheck,
    /// Apply Clippy's machine-applicable lint fixes, then reformat.
    Fix(FixArgs),
    /// Run the test suite (terse). `--verbose` streams logs; `--scope unit|e2e|all` (or
    /// `--e2e`/`--all`).
    Test(TestArgs),
    /// First-time setup of the global pwf shim (`~/.local/bin/pwf` on Unix; scoop on Windows).
    Install,
    /// Rebuild + refresh the installed shim. `--dry` previews; `-f`/`--force` skips the fmt-check
    /// preflight.
    Update(UpdateArgs),
}
