//! Repository automation invoked as `cargo run -p xtask -- <verb>`; the binary is not installed.

use anyhow::Result;
use clap::Parser;

mod architecture_check;
mod child_process;
mod cli;
mod gate;
mod paths;
mod process;
mod sqlite_url;
mod sqlx_cli;
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
        Command::Prepare { check } => verbs::prepare::run(check),
        Command::Fix(arguments) => verbs::format::fix(&arguments.arguments_extra),
        Command::Test(test) => verbs::test::run(test.scope, test.verbose),
        Command::Ship => verbs::ship::run(),
        Command::Install => verbs::install::install(),
        Command::Update(update) => verbs::install::update(&update),
        Command::CheckArchitecture => verbs::check_architecture::run(),
    }
}
