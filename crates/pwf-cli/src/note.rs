//! Owns project-note parsing, request mapping, and execution.

use std::fmt::Write;

use clap::{Args, Subcommand};
use pwf_application::{
    note::{
        add_note::{self, AddNote, AddNoteOk},
        list_notes::{self, ListNotes, ListNotesOk},
        remove_note::{self, RemoveNote, RemoveNoteOk},
        update_note::{self, UpdateNote, UpdateNoteOk},
    },
    ports::clock::Clock,
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
    /// Add a study note from `<title> / <content>` or explicit title and content flags.
    Add {
        /// Managed project (name or id code, case-insensitive).
        #[arg(value_name = "PROJECT")]
        project: String,
        /// Title and Markdown content separated by ` / `.
        #[arg(
            value_name = "NOTE",
            required_unless_present_any = ["title", "content"],
            conflicts_with_all = ["title", "content"]
        )]
        note: Option<String>,
        /// Note title.
        #[arg(long, requires = "content", conflicts_with = "note")]
        title: Option<String>,
        /// Markdown note content.
        #[arg(long, requires = "title", conflicts_with = "note")]
        content: Option<String>,
        /// Why the insight changes future judgment.
        #[arg(long)]
        why: Option<String>,
        /// Subject classification.
        #[arg(long)]
        domain: Option<String>,
        /// Discovery tag; repeat for several.
        #[arg(long = "tag", value_name = "TAG")]
        tags: Vec<String>,
        /// Supporting source or evidence; repeat for several.
        #[arg(long = "source", value_name = "SOURCE")]
        sources: Vec<String>,
        /// Verification date or marker.
        #[arg(long)]
        verified: Option<String>,
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
    /// Replace a note's title: `pwf note update <proj> <id> "<title>"`.
    Update {
        /// Managed project (name or id code, case-insensitive).
        #[arg(value_name = "PROJECT")]
        project: String,
        /// Note id: full `PWF-NOTE-0001`, `NOTE-0001`, or a bare `1`.
        #[arg(value_name = "ID")]
        id: String,
        /// Replacement title words (joined with single spaces).
        #[arg(value_name = "TITLE", required = true)]
        title: Vec<String>,
    },
}

pub async fn run(
    arguments: &Arguments,
    store: &ObsidianStore,
    pool: &sqlx::SqlitePool,
    clock: &impl Clock,
) -> Result<String, String> {
    match &arguments.command {
        Command::List { project, number } => list_notes::execute(
            ListNotes {
                project_identifier: project.clone(),
                number: *number,
            },
            store,
            pool,
        )
        .await
        .map(|result| render_listed(&result))
        .map_err(|error| error.to_string()),
        Command::Add {
            project,
            note,
            title,
            content,
            why,
            domain,
            tags,
            sources,
            verified,
        } => {
            let (title, content) =
                resolve_add_input(note.as_deref(), title.as_deref(), content.as_deref())?;
            add_note::execute(
                AddNote {
                    project_identifier: project.clone(),
                    title: title.to_string(),
                    content: content.to_string(),
                    why: why.clone(),
                    domain: domain.clone(),
                    tags: tags.clone(),
                    sources: sources.clone(),
                    verified: verified.clone(),
                    date: arguments.common.date.clone(),
                },
                store,
                pool,
                clock,
            )
            .await
            .map(|result| render_added(&result))
            .map_err(|error| error.to_string())
        }
        Command::Remove { project, id } => remove_note::execute(
            RemoveNote {
                project_identifier: project.clone(),
                id: id.clone(),
            },
            store,
            pool,
        )
        .await
        .map(|result| render_removed(&result))
        .map_err(|error| error.to_string()),
        Command::Update { project, id, title } => update_note::execute(
            UpdateNote {
                project_identifier: project.clone(),
                id: id.clone(),
                title: title.join(" "),
            },
            store,
            pool,
        )
        .await
        .map(|result| render_updated(&result))
        .map_err(|error| error.to_string()),
    }
}

fn resolve_add_input<'a>(
    note: Option<&'a str>,
    title: Option<&'a str>,
    content: Option<&'a str>,
) -> Result<(&'a str, &'a str), String> {
    match (note, title, content) {
        (Some(note), None, None) => note.split_once(" / ").ok_or_else(|| {
            "Positional note must contain ' / ' between its title and content.".to_string()
        }),
        (None, Some(title), Some(content)) => Ok((title, content)),
        _ => Err("Provide either '<title> / <content>' or both --title and --content.".to_string()),
    }
}

fn render_listed(result: &ListNotesOk) -> String {
    if result.notes.is_empty() {
        return format!("No notes for {}.\n", result.project);
    }
    let mut output = String::new();
    for note in &result.notes {
        let _ = writeln!(output, "{} :: {}", note.id, note.title);
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
    format!("Added {} :: {}\n", result.id, result.title)
}

fn render_removed(result: &RemoveNoteOk) -> String {
    format!("Removed {}\n", result.id)
}

fn render_updated(result: &UpdateNoteOk) -> String {
    format!("Updated {} :: {}\n", result.id, result.title)
}

#[cfg(test)]
mod tests {
    use pwf_application::note::{
        add_note::AddNoteOk, dto::ListedNote, list_notes::ListNotesOk, remove_note::RemoveNoteOk,
        update_note::UpdateNoteOk,
    };
    use pwf_models::{note::NoteId, task::ProjectName};

    use super::{render_added, render_listed, render_removed, render_updated, resolve_add_input};

    fn identifier(number: u32) -> NoteId {
        NoteId::try_new(format!("PWF-NOTE-{number:04}")).unwrap()
    }

    #[test]
    fn typed_results_render_the_existing_note_output_contract() {
        assert_eq!(
            render_added(&AddNoteOk {
                id: identifier(1),
                title: "remember milk".to_string(),
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
                title: "remember oat milk".to_string(),
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
                        title: "second".to_string(),
                    },
                    ListedNote {
                        id: identifier(1),
                        title: "first".to_string(),
                    },
                ],
                hidden: 3,
            }),
            "PWF-NOTE-0002 :: second\nPWF-NOTE-0001 :: first\n... and 3 more; run 'pwf note <proj> ls -n 0' to show all\n"
        );
    }

    #[test]
    fn positional_note_splits_once_on_the_exact_separator() {
        assert_eq!(
            resolve_add_input(
                Some("using join / preserve docs/async.md / and later separators"),
                None,
                None,
            ),
            Ok((
                "using join",
                "preserve docs/async.md / and later separators"
            ))
        );
        assert_eq!(
            resolve_add_input(Some("title/content"), None, None),
            Err("Positional note must contain ' / ' between its title and content.".to_string())
        );
    }
}
