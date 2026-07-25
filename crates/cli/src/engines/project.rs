//! Parses and dispatches managed-project commands.

use clap::{Args, Subcommand};
use pwf_domain::project::ProjectPrefix;
use pwf_infra::SqliteStore;

pub mod add;
pub mod get;
pub mod list;
mod output;
pub mod pause;
pub mod resume;

#[derive(Args, Debug)]
pub struct Arguments {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Lists registered projects in ascending title order.
    #[command(name = "ls")]
    List(list::Arguments),
    /// Gets one registered project.
    Get(get::Arguments),
    /// Adds one registered project.
    Add(add::Arguments),
    /// Pauses one registered project.
    Pause(pause::Arguments),
    /// Resumes one registered project.
    Resume(resume::Arguments),
}

pub async fn run(arguments: Arguments, database: &SqliteStore) -> Result<String, String> {
    let Some(command) = arguments.command else {
        return Ok(crate::command::project_help());
    };

    match command {
        Command::List(arguments) => list::run(arguments, database).await,
        Command::Get(arguments) => get::run(arguments, database).await,
        Command::Add(arguments) => add::run(arguments, database).await,
        Command::Pause(arguments) => pause::run(arguments, database).await,
        Command::Resume(arguments) => resume::run(arguments, database).await,
    }
}

fn parse_project_id(raw: &str) -> Result<ProjectPrefix, String> {
    ProjectPrefix::try_new(raw)
        .map_err(|_| "project id must contain two to four ASCII letters".to_string())
}
