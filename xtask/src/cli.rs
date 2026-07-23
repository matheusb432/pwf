//! Command-line surface. clap derives `--help` from the doc comments here and on each verb's
//! `Args` struct — keep them the single source of truth for the verb documentation.

use clap::{Parser, Subcommand};

use crate::{
    verb::Verb,
    verbs::{format::FixArguments, install::UpdateArgs, test::TestArgs},
};

/// pwf's embedded dev/release automation (xtask). Never installed — run via `cargo run -p xtask`.
#[derive(Parser)]
#[command(
    version,
    about = "pwf's embedded dev/release automation (xtask)",
    styles = clap_cargo::style::CLAP_STYLING
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

/// One arm per automation verb, each dispatched to its [`crate::verbs`] module. Let clap validate
/// the surface — `ValueEnum` for closed choices, `conflicts_with` for exclusive flags.
#[derive(Subcommand)]
pub(crate) enum Command {
    /// Format the repo in place (stable/pinned-nightly rustfmt; rumdl when `.rumdl.toml`
    /// present).
    #[command(name = Verb::FORMAT.as_str())]
    Format,
    /// Check formatting without writing; exits non-zero on drift.
    #[command(name = Verb::FORMAT_CHECK.as_str())]
    FormatCheck,
    /// Run repository linters.
    #[command(name = Verb::LINT.as_str())]
    Lint,
    /// Run the complete read-only formatting and lint gate.
    #[command(name = Verb::CHECK.as_str())]
    Check,
    /// Apply Clippy's machine-applicable lint fixes, then reformat.
    #[command(name = Verb::FIX.as_str())]
    Fix(FixArguments),
    /// Run the test suite (terse). `--verbose` streams logs; `--scope unit|e2e|all` (or
    /// `--e2e`/`--all`).
    #[command(name = Verb::TEST.as_str())]
    Test(TestArgs),
    /// Run the full test preflight and build the release binary.
    #[command(name = Verb::SHIP.as_str())]
    Ship,
    /// First-time setup of the global pwf shim (`~/.local/bin/pwf` on Unix; scoop on Windows).
    #[command(name = Verb::INSTALL.as_str())]
    Install,
    /// Rebuild + refresh the installed shim. `--dry` previews; `-f`/`--force` skips the full check
    /// preflight.
    #[command(name = Verb::UPDATE.as_str())]
    Update(UpdateArgs),
    /// Fail if any `crates/cli` source file outside the composition-root allowlist imports the
    /// infra crate directly; violations print as `<path>:<line>: <message>` on stderr.
    #[command(name = Verb::CHECK_ARCHITECTURE.as_str())]
    CheckArchitecture,
}
