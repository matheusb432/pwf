//! Automatic quality fixes.

use anyhow::Result;
use clap::Args;

use super::{format, lint};
use crate::{
    process::{self, Status},
    task,
    verb::Verb,
};

#[derive(Args)]
pub(crate) struct FixArguments {
    /// Extra arguments for `cargo clippy --fix`.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub(crate) arguments_extra: Vec<String>,
}

pub(crate) fn run(arguments_extra: &[String]) -> Result<()> {
    task::run_all(&[lint::fix_step(arguments_extra)])?;
    task::run_all(&format::write_steps()?)?;
    process::result(Verb::FIX, Status::Done);
    Ok(())
}
