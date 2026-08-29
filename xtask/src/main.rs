use anyhow::Result;
use clap::Parser;

mod child_process;
mod cli;
mod paths;
mod process;
mod sqlite_url;
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
        Command::Prepare { check } => verbs::prepare::run(check),
        Command::Ship(arguments) => verbs::ship::run(&arguments),
        Command::Install => verbs::install::install(),
        Command::Update(update) => verbs::install::update(&update),
        Command::CheckArchitecture { root } => verbs::check_architecture::run(root.as_deref()),
    }
}
