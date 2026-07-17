//! Embedded development and release automation.
//!
//! Invoke it as `cargo run -p xtask -- <verb>`. The binary is not installed.

use anyhow::Result;
use clap::Parser;

mod architecture_check;
mod cli;
mod paths;
mod proc;
mod task;
mod verbs;

fn main() {
    if let Err(e) = run(cli::Cli::parse().command) {
        eprintln!("Error: {e:#}");
        std::process::exit(1);
    }
}

fn run(command: cli::Command) -> Result<()> {
    use cli::Command;
    match command {
        Command::Fmt => verbs::fmt::fmt(),
        Command::FmtCheck => verbs::fmt::fmt_check(),
        Command::Fix(fix) => verbs::fmt::fix(&fix.args),
        Command::Test(test) => verbs::test::run(test.scope, test.verbose),
        Command::Install => verbs::install::install(),
        Command::Update(update) => verbs::install::update(&update),
        Command::CheckArchitecture => verbs::check_architecture::run(),
    }
}
