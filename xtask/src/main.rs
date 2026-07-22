//! Embedded development and release automation.
//!
//! Invoke it as `cargo run -p xtask -- <verb>`. The binary is not installed.

use anyhow::Result;
use clap::Parser;

mod architecture_check;
mod cli;
mod paths;
mod process;
mod task;
mod verb;
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
        Command::Format => verbs::format::run(),
        Command::FormatCheck => verbs::format::check(),
        Command::Lint => verbs::lint::run(),
        Command::Check => verbs::check::run(),
        Command::Fix(arguments) => verbs::format::fix(&arguments.arguments_extra),
        Command::Test(test) => verbs::test::run(test.scope, test.verbose),
        Command::Ship => verbs::ship::run(),
        Command::Install => verbs::install::install(),
        Command::Update(update) => verbs::install::update(&update),
        Command::CheckArchitecture => verbs::check_architecture::run(),
    }
}
