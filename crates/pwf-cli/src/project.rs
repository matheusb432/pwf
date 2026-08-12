//! Parses and dispatches managed-project commands.

use std::path::PathBuf;

use clap::{Args, Subcommand};
use pwf_models::project::{ProjectId, ProjectName, ProjectSourceValue, ProjectTasksPath};
use sqlx::SqlitePool;

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
    /// Renames one registered project and its task files.
    Rename(rename::Arguments),
    /// Resumes one registered project.
    Resume(resume::Arguments),
}

pub async fn run(
    arguments: Arguments,
    pool: &SqlitePool,
    home: Option<PathBuf>,
) -> Result<String, String> {
    let Some(command) = arguments.command else {
        return Ok(crate::command::project_help());
    };

    match command {
        Command::List(arguments) => list::run(arguments, pool).await,
        Command::Get(arguments) => get::run(arguments, pool).await,
        Command::Add(arguments) => add::run(arguments, pool, project_home(home)?).await,
        Command::Pause(arguments) => pause::run(arguments, pool).await,
        Command::Rename(arguments) => rename::run(arguments, pool, project_home(home)?).await,
        Command::Resume(arguments) => resume::run(arguments, pool, project_home(home)?).await,
    }
}

fn project_home(home: Option<PathBuf>) -> Result<PathBuf, String> {
    home.ok_or_else(|| "resolving the home directory for managed projects failed".to_string())
}

fn parse_project_id(raw: &str) -> Result<ProjectId, String> {
    ProjectId::try_new(raw)
        .map_err(|_| "project id must contain two to four ASCII letters".to_string())
}

fn parse_project_title(raw: &str) -> Result<ProjectName, String> {
    ProjectName::try_new(raw).map_err(|error| error.to_string())
}

fn parse_project_source(raw: &str) -> Result<ProjectSourceValue, String> {
    ProjectSourceValue::try_new(raw)
        .map_err(|_| "project source value must not be blank".to_string())
}

fn parse_project_tasks(raw: &str) -> Result<ProjectTasksPath, String> {
    ProjectTasksPath::try_new(raw).map_err(|_| "project tasks path must not be blank".to_string())
}
