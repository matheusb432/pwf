use std::path::PathBuf;

use clap::{Args, Subcommand};
use pwf_application::ports::clock::Clock;
use pwf_infra::obsidian::ObsidianStore;

use crate::{console::Console, task::shared::TaskError};

pub mod add;
pub mod cancel;
pub mod done;
pub mod edit;
pub mod list;
pub mod remove;
mod render;
pub mod reopen;
pub mod route;
pub mod session;
pub(crate) mod shared;
pub mod show;

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Manages pwf tasks
    Task(TaskArguments),
    /// Dispatch an agent session into a project's cwd, via tmux session or inline
    Session(session::Arguments),
    /// Internal word router behind bare `pwf <words...>`
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
    /// Add a pwf task with shorthand prompt text or explicit args
    Add(add::Arguments),
    /// List tasks. `--all` lists everything
    #[command(alias = "ls")]
    List(list::Arguments),
    /// Mark a task done in place, keeping a capped done-queue
    Done(done::Arguments),
    /// Mark a task cancelled in place, keeping the same capped queue as done
    Cancel(cancel::Arguments),
    /// Reopen a closed task. This removes it's completed/commits provenance.
    Reopen(reopen::Arguments),
    /// Edit an active task's prompt body, title, prerequisites, tags, or effort
    Edit(edit::Arguments),
    /// Show a task note's markdown
    #[command(alias = "s")]
    Show(show::Arguments),
    /// Delete a task note and remove its index link
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
            TaskCommand::Edit(arguments) => edit::run(arguments, console, store, pool).await,
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
