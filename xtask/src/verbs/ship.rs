use std::process::Command;

use anyhow::Result;
use clap::Args;

use crate::{process, verbs::test};

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
        test::run_all()?;
    }
    process::run(
        "release build",
        Command::new("cargo").args(["build", "--release"]),
    )
}
