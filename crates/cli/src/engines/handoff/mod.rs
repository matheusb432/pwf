//! Implements handoff creation and listing.

use clap::Subcommand;

pub mod add;
mod common;
pub mod list;

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Create a handoff (allocates its linked pw item).
    Add(add::Arguments),
    /// List handoffs.
    List(list::Arguments),
}

pub fn run(command: &Command) -> Result<String, String> {
    match command {
        Command::Add(arguments) => add::run(arguments),
        Command::List(arguments) => list::run(arguments),
    }
    .map_err(|error| error.to_string())
}
