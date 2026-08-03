use std::path::PathBuf;

use clap::{Args, Subcommand};
use pwf_application::ports::clock::Clock;
use pwf_infra::obsidian::ObsidianStore;

use crate::{console::Console, task::shared::TaskError};

pub mod add;
pub mod cancel;
pub mod done;
pub mod list;
pub mod remove;
mod render;
pub mod reopen;
pub mod route;
pub mod session;
pub(crate) mod shared;
pub mod show;
pub mod update;

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Manages pwf tasks.
    Task(TaskArguments),
    /// Dispatch a real agent session into the project's tmux session as a new window.
    ///
    /// The id is a bare positional (`pwf session <id>`) or `--id`.
    Session(session::Arguments),
    /// Internal word router behind bare `pwf <words...>`.
    #[command(hide = true)]
    Route(route::Arguments),
}

#[derive(Args, Debug)]
pub struct TaskArguments {
    #[command(subcommand)]
    command: TaskCommand,
}

#[derive(Subcommand, Debug)]
enum TaskCommand {
    /// Add a pwf task: `pwf task add <project> "<prompt>"`.
    ///
    /// Prompt words are joined with single spaces, so quotes are optional. Rich
    /// prompts use lanes: `<title> / <goal> /c <context> /n <constraint> /d <done>`.
    /// The title is stored separately from Goals and has a 200-character limit after normalization.
    /// `--continue <path>` builds the prompt from a plan path instead of positional words.
    Add(add::Arguments),
    /// List tasks (active only, capped, scoped sections hidden; `--all` lists
    /// everything).
    #[command(alias = "ls")]
    List(list::Arguments),
    /// Mark a task done in place, keeping a capped done-queue.
    Done(done::Arguments),
    /// Mark a task cancelled in place, keeping the same capped queue as done.
    Cancel(cancel::Arguments),
    /// Reopen a closed task: flip done/cancelled back to active, drop its
    /// completed/commits provenance, and restore an existing index link.
    Reopen(reopen::Arguments),
    /// Edit a task's prompt body, title, prereqs, tags, effort, or provenance.
    ///
    /// Replacement titles have a 200-character limit after normalization.
    ///
    /// Only `--commits` and `--append-report` are allowed on a closed task.
    Update(update::Arguments),
    /// Stream a task note's markdown (any status, incl. archived done/cancelled).
    ///
    /// The id is a bare positional (`pwf task show <id>`) or `--id`. `s` is an
    /// alias. `--path` prints the note path; `--json` prints typed task data.
    #[command(alias = "s")]
    Show(show::Arguments),
    /// Delete a task note and remove its index link.
    Remove(remove::Arguments),
}

pub async fn run(
    command: &Command,
    console: Console,
    store: &ObsidianStore,
    pool: &sqlx::SqlitePool,
    home: &PathBuf,
    clock: &impl Clock,
) -> Result<String, String> {
    match command {
        Command::Task(arguments) => match &arguments.command {
            TaskCommand::Add(arguments) => add::run(arguments, console, store, pool, clock).await,
            TaskCommand::List(arguments) => list::run(arguments, console, store, pool).await,
            TaskCommand::Done(arguments) => done::run(arguments, store, pool, clock).await,
            TaskCommand::Cancel(arguments) => cancel::run(arguments, store, pool, clock).await,
            TaskCommand::Reopen(arguments) => reopen::run(arguments, store, pool).await,
            TaskCommand::Update(arguments) => update::run(arguments, console, store, pool).await,
            TaskCommand::Show(arguments) => show::run(arguments, store, pool).await,
            TaskCommand::Remove(arguments) => remove::run(arguments, console, store, pool).await,
        },
        Command::Session(arguments) => session::run(arguments, console, store, pool, home).await,
        Command::Route(arguments) => match route::resolve(arguments) {
            route::ResolvedCommand::List(arguments) => {
                list::run(&arguments, console, store, pool).await
            }
            route::ResolvedCommand::RejectUnsupportedTaskCreation => {
                Err(TaskError::RejectUnsupportedTaskCreation)
            }
        },
    }
    .map_err(String::from)
}
