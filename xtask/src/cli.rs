use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::verbs::{Verb, install::UpdateArgs, ship::ShipArguments};

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

#[derive(Subcommand)]
pub(crate) enum Command {
    /// Refresh the committed `SQLx` checked-query cache, or verify it with `--check`.
    #[command(name = Verb::PREPARE.as_str())]
    Prepare {
        /// Verify that committed query metadata matches the checked queries.
        #[arg(long)]
        check: bool,
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
