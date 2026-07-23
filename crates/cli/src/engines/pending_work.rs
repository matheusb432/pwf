use clap::Subcommand;

use crate::console::Console;

pub mod add;
pub mod cancel;
pub(crate) mod common;
pub mod done;
pub mod list;
pub mod remove;
mod render;
pub mod reopen;
pub mod route;
pub mod session;
pub mod show;
pub mod update;
pub mod verify;

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Add a pwf task: `pwf add <project> "<prompt>"`.
    ///
    /// Prompt words are joined with single spaces, so quotes are optional. Rich
    /// prompts use lanes: `<title> / <goal> /c <context> /n <constraint> /d <done>`.
    /// `--continue-handoff` / `--continue <path>` build the prompt from the
    /// repo's newest handoff or a plan path instead of positional words.
    Add(add::Arguments),
    /// List pending-work items (active only, capped, scoped sections hidden; `--all` lists
    /// everything).
    #[command(alias = "ls")]
    List(list::Arguments),
    /// Mark an item done in place, keeping a capped done-queue.
    Done(done::Arguments),
    /// Mark an item cancelled in place, keeping the same capped queue as done.
    Cancel(cancel::Arguments),
    /// Reopen a closed item: flip done/cancelled back to active, drop its
    /// completed/commits provenance, and restore its index link.
    Reopen(reopen::Arguments),
    /// Replace an item's prompt body and/or title; append or clear its prereqs;
    /// splice rich lane-syntax bullets into the body; amend its `commits:`
    /// provenance; or append a closeout report; the last two are the only edits
    /// allowed on a closed item.
    Update(update::Arguments),
    /// Stream a task note's markdown (any status, incl. archived done/cancelled).
    ///
    /// The id is a bare positional (`pwf show <id>`) or `--id`. `pwf s` is
    /// an alias. `--path` prints the note's path instead of its markdown.
    #[command(alias = "s")]
    Show(show::Arguments),
    /// Probe whether an agent is launchable.
    Verify(verify::Arguments),
    /// Internal word router behind bare `pwf <words...>`.
    #[command(hide = true)]
    Route(route::Arguments),
    /// Delete a task note and remove its index link.
    Remove(remove::Arguments),
    /// Dispatch a real agent session into the item's zellij session as a new tab.
    ///
    /// The id is a bare positional (`pwf session <id>`) or `--id`.
    Session(session::Arguments),
}

pub fn run(command: &Command, console: Console) -> Result<String, String> {
    match command {
        Command::Add(arguments) => add::run(arguments, console),
        Command::List(arguments) => list::run(arguments, console),
        Command::Done(arguments) => done::run(arguments),
        Command::Cancel(arguments) => cancel::run(arguments),
        Command::Reopen(arguments) => reopen::run(arguments),
        Command::Update(arguments) => update::run(arguments, console),
        Command::Show(arguments) => show::run(arguments),
        Command::Verify(arguments) => verify::run(arguments),
        Command::Route(arguments) => match route::resolve(arguments) {
            route::ResolvedCommand::List(arguments) => list::run(&arguments, console),
            route::ResolvedCommand::Verify(arguments) => verify::run(&arguments),
        },
        Command::Remove(arguments) => remove::run(arguments, console),
        Command::Session(arguments) => session::run(arguments, console),
    }
    .map_err(String::from)
}
