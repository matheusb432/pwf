//! Release workflow.

use anyhow::Result;
use clap::Args;

use crate::{
    process::{self, Status},
    verb::Verb,
    verbs::test,
};

#[derive(Args)]
pub(crate) struct ShipArguments {
    /// Skip the test preflight.
    #[arg(short = 'f', long)]
    pub(crate) force: bool,
}

pub(crate) fn run(force: bool) -> Result<()> {
    if force {
        eprintln!("ship: --force - skipping the test preflight");
    } else {
        test::run_all()?;
    }
    process::run("release build", "cargo", &["build", "--release"])?;
    process::result(Verb::SHIP, Status::Pass);
    Ok(())
}
