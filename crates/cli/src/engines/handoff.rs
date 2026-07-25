//! Implements handoff creation and listing.

use clap::Subcommand;
use pwf_application::{Clock, pending_work::ProjectRegistry};
use pwf_infra::obsidian::ObsidianStore;

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

pub fn run<C>(
    command: &Command,
    store: &ObsidianStore,
    projects: &ProjectRegistry,
    clock: &C,
) -> Result<String, String>
where
    C: Clock,
{
    match command {
        Command::Add(arguments) => add::run(arguments, store, projects, clock),
        Command::List(arguments) => list::run(arguments, store),
    }
    .map_err(|error| error.to_string())
}
