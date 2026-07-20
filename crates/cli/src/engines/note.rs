//! Owns project-note parsing, request mapping, and execution.

use clap::{Args, Subcommand};
use pwf_note::{NoteCommand, NoteVerb};

#[derive(Args, Debug)]
pub struct Arguments {
    #[command(subcommand)]
    pub(crate) command: Command,
    #[command(flatten)]
    common: CommonArguments,
}

#[derive(Args, Debug, Default)]
struct CommonArguments {
    /// Path to the pwf config JSON (overrides $`PWF_CONFIG`).
    #[arg(long, global = true)]
    config_path: Option<String>,
    /// Override the notes directory.
    #[arg(long, global = true)]
    notes_dir: Option<String>,
    /// Date stamp (YYYY-MM-DD); defaults to today.
    #[arg(long, global = true)]
    date: Option<String>,
}

#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    /// List a project's notes, newest-first (`pwf note <proj>` alone also lists).
    #[command(alias = "ls")]
    List {
        /// Managed project (name or id code, case-insensitive).
        #[arg(value_name = "PROJECT")]
        project: String,
        /// Cap to N listed notes (default 10; `-n 0` = all).
        #[arg(short = 'n', long, value_name = "N")]
        number: Option<usize>,
    },
    /// Add a one-liner note: `pwf note add <proj> "<message>"`.
    Add {
        /// Managed project (name or id code, case-insensitive).
        #[arg(value_name = "PROJECT")]
        project: String,
        /// Note message words (joined with single spaces).
        #[arg(value_name = "MESSAGE", required = true)]
        message: Vec<String>,
    },
    /// Delete a note and strip its index link: `pwf note remove <proj> <id>`.
    Remove {
        /// Managed project (name or id code, case-insensitive).
        #[arg(value_name = "PROJECT")]
        project: String,
        /// Note id: full `PWF-NOTE-0001`, `NOTE-0001`, or a bare `1`.
        #[arg(value_name = "ID")]
        id: String,
    },
    /// Replace a note's message: `pwf note update <proj> <id> "<message>"`.
    Update {
        /// Managed project (name or id code, case-insensitive).
        #[arg(value_name = "PROJECT")]
        project: String,
        /// Note id: full `PWF-NOTE-0001`, `NOTE-0001`, or a bare `1`.
        #[arg(value_name = "ID")]
        id: String,
        /// Replacement note message words (joined with single spaces).
        #[arg(value_name = "MESSAGE", required = true)]
        message: Vec<String>,
    },
}

pub fn run(arguments: &Arguments) -> Result<String, String> {
    pwf_note::run(&application_command(arguments))
}

fn application_command(arguments: &Arguments) -> NoteCommand {
    let (project, verb) = match &arguments.command {
        Command::List { project, number } => (project.clone(), NoteVerb::Ls { number: *number }),
        Command::Add { project, message } => (
            project.clone(),
            NoteVerb::Add {
                message: message.join(" "),
            },
        ),
        Command::Remove { project, id } => (project.clone(), NoteVerb::Remove { id: id.clone() }),
        Command::Update {
            project,
            id,
            message,
        } => (
            project.clone(),
            NoteVerb::Update {
                id: id.clone(),
                message: message.join(" "),
            },
        ),
    };
    NoteCommand {
        project,
        verb,
        config_path: arguments.common.config_path.clone(),
        notes_dir: arguments.common.notes_dir.clone(),
        date: arguments.common.date.clone(),
    }
}
