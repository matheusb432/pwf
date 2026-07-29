//! Owns project-note parsing, request mapping, and execution.

use std::fmt::Write;

use clap::{Args, Subcommand};
use pwf_application::{
    Clock,
    note::{
        add_note::{self, AddNote, AddNoteOk},
        list_notes::{self, ListNotes, ListNotesOk},
        remove_note::{self, RemoveNote, RemoveNoteOk},
        update_note::{self, UpdateNote, UpdateNoteOk},
    },
    pending_work::ProjectRegistry,
};
use pwf_infra::obsidian::ObsidianStore;

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

pub fn run<C>(
    arguments: &Arguments,
    store: &ObsidianStore,
    projects: &ProjectRegistry,
    clock: &C,
) -> Result<String, String>
where
    C: Clock,
{
    match &arguments.command {
        Command::List { project, number } => list_notes::execute(
            ListNotes {
                project_identifier: project.clone(),
                number: *number,
            },
            store,
            projects,
        )
        .map(|result| render_listed(&result))
        .map_err(|error| error.to_string()),
        Command::Add { project, message } => add_note::execute(
            AddNote {
                project_identifier: project.clone(),
                message: message.join(" "),
                date: arguments.common.date.clone(),
            },
            store,
            projects,
            clock,
        )
        .map(|result| render_added(&result))
        .map_err(|error| error.to_string()),
        Command::Remove { project, id } => remove_note::execute(
            RemoveNote {
                project_identifier: project.clone(),
                id: id.clone(),
            },
            store,
            projects,
        )
        .map(|result| render_removed(&result))
        .map_err(|error| error.to_string()),
        Command::Update {
            project,
            id,
            message,
        } => update_note::execute(
            UpdateNote {
                project_identifier: project.clone(),
                id: id.clone(),
                message: message.join(" "),
            },
            store,
            projects,
        )
        .map(|result| render_updated(&result))
        .map_err(|error| error.to_string()),
    }
}

fn render_listed(result: &ListNotesOk) -> String {
    if result.notes.is_empty() {
        return format!("No notes for {}.\n", result.project);
    }
    let mut output = String::new();
    for note in &result.notes {
        let _ = writeln!(output, "{} :: {}", note.id, note.message);
    }
    if result.hidden > 0 {
        let _ = writeln!(
            output,
            "... and {} more; run 'pwf note <proj> ls -n 0' to show all",
            result.hidden
        );
    }
    output
}

fn render_added(result: &AddNoteOk) -> String {
    format!("Added {} :: {}\n", result.id, result.message)
}

fn render_removed(result: &RemoveNoteOk) -> String {
    format!("Removed {}\n", result.id)
}

fn render_updated(result: &UpdateNoteOk) -> String {
    format!("Updated {} :: {}\n", result.id, result.message)
}

#[cfg(test)]
mod tests {
    use pwf_application::note::{
        add_note::AddNoteOk, dto::ListedNote, list_notes::ListNotesOk, remove_note::RemoveNoteOk,
        update_note::UpdateNoteOk,
    };
    use pwf_models::{note::NoteId, pending_work::ProjectName};

    use super::{render_added, render_listed, render_removed, render_updated};

    fn identifier(number: u32) -> NoteId {
        NoteId::try_new(format!("PWF-NOTE-{number:04}")).unwrap()
    }

    #[test]
    fn typed_results_render_the_existing_note_output_contract() {
        assert_eq!(
            render_added(&AddNoteOk {
                id: identifier(1),
                message: "remember milk".to_string(),
            }),
            "Added PWF-NOTE-0001 :: remember milk\n"
        );
        assert_eq!(
            render_removed(&RemoveNoteOk { id: identifier(1) }),
            "Removed PWF-NOTE-0001\n"
        );
        assert_eq!(
            render_updated(&UpdateNoteOk {
                id: identifier(1),
                message: "remember oat milk".to_string(),
            }),
            "Updated PWF-NOTE-0001 :: remember oat milk\n"
        );
    }

    #[test]
    fn listed_results_render_empty_lines_and_hidden_hint() {
        let project = ProjectName::try_new("pwf").unwrap();
        assert_eq!(
            render_listed(&ListNotesOk {
                project: project.clone(),
                notes: Vec::new(),
                hidden: 0,
            }),
            "No notes for pwf.\n"
        );
        assert_eq!(
            render_listed(&ListNotesOk {
                project,
                notes: vec![
                    ListedNote {
                        id: identifier(2),
                        message: "second".to_string(),
                    },
                    ListedNote {
                        id: identifier(1),
                        message: "first".to_string(),
                    },
                ],
                hidden: 3,
            }),
            "PWF-NOTE-0002 :: second\nPWF-NOTE-0001 :: first\n... and 3 more; run 'pwf note <proj> ls -n 0' to show all\n"
        );
    }
}
