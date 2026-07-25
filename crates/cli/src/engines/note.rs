//! Owns project-note parsing, request mapping, and execution.

use clap::{Args, Subcommand};
use pwf_application::pending_work::ProjectRegistry;
use pwf_infra::obsidian::ObsidianStore;
use pwf_note::{NoteCommand, NoteTarget, NoteVerb};

#[derive(Args, Debug)]
pub struct Arguments {
    #[command(subcommand)]
    pub(crate) command: Command,
    #[command(flatten)]
    common: CommonArguments,
}

#[derive(Args, Debug, Default)]
struct CommonArguments {
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

pub fn run(
    arguments: &Arguments,
    store: &ObsidianStore,
    projects: &ProjectRegistry,
) -> Result<String, String> {
    pwf_note::run(&application_command(arguments, store, projects)?)
}

fn application_command(
    arguments: &Arguments,
    store: &ObsidianStore,
    projects: &ProjectRegistry,
) -> Result<NoteCommand, String> {
    let (raw_project, verb) = match &arguments.command {
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
    let project = projects
        .resolve(&raw_project)
        .map_err(|_| unknown_project(&raw_project))?
        .clone();
    let prefix = projects
        .prefix_for(&project)
        .ok_or_else(|| unknown_project(&raw_project))?;
    let target = NoteTarget {
        tasks_path: store
            .tasks_path(&project)
            .map_err(|error| error.to_string())?
            .to_path_buf(),
        project,
        prefix,
    };

    Ok(NoteCommand {
        target,
        verb,
        date: arguments.common.date.clone(),
    })
}

fn unknown_project(raw_project: &str) -> String {
    format!("Unknown project '{raw_project}'; expected a managed project name or id code.")
}
