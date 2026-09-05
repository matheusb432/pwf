use std::process::Command;

use anyhow::Result;
use clap::Args;

use crate::process;

#[derive(Args)]
pub(crate) struct ShipArguments {
    /// Skip the test preflight.
    #[arg(short = 'f', long)]
    force: bool,
}

pub(crate) fn run(arguments: &ShipArguments) -> Result<()> {
    if arguments.force {
        eprintln!("ship: --force - skipping the test preflight");
    } else {
        process::run("test preflight", Command::new("just").arg("test-all"))?;
    }
    process::run(
        "release build",
        Command::new("cargo").args(["build", "--release", "-p", "pwf-cli", "-p", "pwf-server"]),
    )
}
