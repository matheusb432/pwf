//! Parses and dispatches managed-project commands.

use std::path::PathBuf;

use clap::{Args, Subcommand};
use pwf_infra::SqliteStore;
use pwf_models::project::ProjectPrefix;

pub mod add;
pub mod get;
pub mod list;
mod output;
pub mod pause;
pub mod rename;
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
    /// Renames one registered project and its pending-work files.
    Rename(rename::Arguments),
    /// Resumes one registered project.
    Resume(resume::Arguments),
}

pub async fn run(
    arguments: Arguments,
    database: &SqliteStore,
    home: Option<PathBuf>,
) -> Result<String, String> {
    let Some(command) = arguments.command else {
        return Ok(crate::command::project_help());
    };

    match command {
        Command::List(arguments) => list::run(arguments, database).await,
        Command::Get(arguments) => get::run(arguments, database).await,
        Command::Add(arguments) => add::run(arguments, database, project_home(home)?).await,
        Command::Pause(arguments) => pause::run(arguments, database).await,
        Command::Rename(arguments) => rename::run(arguments, database, project_home(home)?).await,
        Command::Resume(arguments) => resume::run(arguments, database, project_home(home)?).await,
    }
}

fn project_home(home: Option<PathBuf>) -> Result<PathBuf, String> {
    home.ok_or_else(|| "resolving the home directory for managed projects failed".to_string())
}

fn parse_project_id(raw: &str) -> Result<ProjectPrefix, String> {
    ProjectPrefix::try_new(raw)
        .map_err(|_| "project id must contain two to four ASCII letters".to_string())
}
