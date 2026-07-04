//! `xtask` — pwf's embedded dev/release automation harness.
//!
//! A workspace member built on demand, invoked from the justfile as
//! `cargo run -p xtask -- <verb>`; never installed. Recipe bodies stay one-line forwarders;
//! automation logic lives in [`verbs`].

use anyhow::Result;
use clap::Parser;

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

/// Dispatch one parsed verb to its handler.
fn run(command: cli::Command) -> Result<()> {
    use cli::Command;
    match command {
        Command::Fmt => verbs::fmt::fmt(),
        Command::FmtCheck => verbs::fmt::fmt_check(),
        Command::Fix(fix) => verbs::fmt::fix(&fix.args),
        Command::SmellCheckErrors => verbs::smell_check::run(),
        Command::Test(test) => verbs::test::run(test.scope, test.verbose),
        Command::Install => verbs::install::install(),
        Command::Update(update) => verbs::install::update(&update),
    }
}
