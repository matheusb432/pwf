//! clap derives help from this module and each verb's `Args` doc comments.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::{
    verb::Verb,
    verbs::{fix::FixArguments, install::UpdateArgs, ship::ShipArguments, test::TestArgs},
};

/// pwf's embedded dev/release automation (xtask). Never installed; run via `cargo run -p xtask`.
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
/// the surface: `ValueEnum` for closed choices, `conflicts_with` for exclusive flags.
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
    /// Refresh the committed `SQLx` checked-query cache, or verify it with `--check`.
    #[command(name = Verb::PREPARE.as_str())]
    Prepare {
        /// Verify that committed query metadata matches the checked queries.
        #[arg(long)]
        check: bool,
    },
    /// Apply Clippy's machine-applicable lint fixes, then reformat.
    #[command(name = Verb::FIX.as_str())]
    Fix(FixArguments),
    /// Run the test suite (terse). `--verbose` streams logs; `--scope unit|e2e|all` (or
    /// `--e2e`/`--all`).
    #[command(name = Verb::TEST.as_str())]
    Test(TestArgs),
    /// Build the release binary when needed, then run the binary suites.
    #[command(hide = true)]
    E2eWorker {
        /// Stream Cargo test output from the worker.
        #[arg(long)]
        verbose: bool,
    },
    /// Run the release preflight and build the release binary.
    #[command(name = Verb::SHIP.as_str())]
    Ship(ShipArguments),
    /// First-time setup of the global pwf binary (`~/.local/bin/pwf` on Unix; Scoop on Windows).
    #[command(name = Verb::INSTALL.as_str())]
    Install,
    /// Rebuild + refresh the installed binary. `--dry` previews; `-f`/`--force` skips the full
    /// check preflight.
    #[command(name = Verb::UPDATE.as_str())]
    Update(UpdateArgs),
    /// Reject forbidden outward workspace dependency edges.
    #[command(name = Verb::CHECK_ARCHITECTURE.as_str())]
    CheckArchitecture {
        /// Workspace root to inspect [default: this repository].
        root: Option<PathBuf>,
    },
}
