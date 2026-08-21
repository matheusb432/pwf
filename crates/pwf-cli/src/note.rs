//! Owns project-note parsing, request mapping, and rendering.

use std::{fmt::Write, str::FromStr};

use clap::{Args, Subcommand};
use pwf_client::{
    note::NoteClient,
    v1::{
        AddNoteRequest, AddedNote, ListNotesRequest, ListedNotes, NoteListLimitKind,
        RemoveNoteRequest, RemovedNote, UpdateNoteRequest, UpdatedNote,
    },
};
use pwf_models::{
    AppDate,
    note::{
        NoteContent, NoteDomain, NoteSelector, NoteSource, NoteTag, NoteTitle, NoteVerification,
        NoteWhy,
    },
    project::ProjectSelector,
};

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
    date: Option<AppDate>,
}

#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    /// List a project's notes
    #[command(alias = "ls")]
    List {
        /// Managed project name or id
        #[arg(value_name = "PROJECT")]
        project: ProjectSelector,
        /// Cap to N listed notes (default 10)
        #[arg(short = 'n', long, value_name = "N")]
        number: Option<usize>,
    },
    /// Add a study note from `<title> / <content>` or explicit title and content flags
    Add {
        /// Managed project name or id
        #[arg(value_name = "PROJECT")]
        project: ProjectSelector,
        /// Title and Markdown content separated by ` / `
        #[arg(
            value_name = "NOTE",
            required_unless_present_any = ["title", "content"],
            conflicts_with_all = ["title", "content"]
        )]
        note: Option<PositionalNote>,
        /// Note title
        #[arg(long, requires = "content", conflicts_with = "note")]
        title: Option<NoteTitle>,
        /// Markdown note content
        #[arg(long, requires = "title", conflicts_with = "note")]
        content: Option<NoteContent>,
        /// Why the insight changes future judgment
        #[arg(long)]
        why: Option<NoteWhy>,
        /// Subject classification
        #[arg(long)]
        domain: Option<NoteDomain>,
        /// Discovery tag
        #[arg(long = "tag", value_name = "TAG")]
        tags: Vec<NoteTag>,
        /// Supporting source or evidence; repeat for several
        #[arg(long = "source", value_name = "SOURCE")]
        sources: Vec<NoteSource>,
        /// Verification date or marker
        #[arg(long)]
        verified: Option<NoteVerification>,
    },
    /// Delete a note and strip its index link: `pwf note remove <proj> <id>`
    Remove {
        /// Managed project name or id
        #[arg(value_name = "PROJECT")]
        project: ProjectSelector,
        /// Note id: full `PWF-NOTE-0001`, `NOTE-0001`, or a bare `1`
        #[arg(value_name = "ID")]
        id: NoteSelector,
    },
    /// Replace a note's title: `pwf note update <proj> <id> "<title>"`
    Update {
        /// Managed project name or id
        #[arg(value_name = "PROJECT")]
        project: ProjectSelector,
        /// Note id: full `PWF-NOTE-0001`, `NOTE-0001`, or a bare `1`
        #[arg(value_name = "ID")]
        id: NoteSelector,
        /// Replacement title words
        #[arg(value_name = "TITLE", required = true, num_args = 1..)]
        title: Vec<String>,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct PositionalNote {
    title: NoteTitle,
    content: NoteContent,
}

impl FromStr for PositionalNote {
    type Err = String;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let (title, content) = raw.split_once(" / ").ok_or_else(|| {
            "Positional note must contain ' / ' between its title and content.".to_string()
        })?;
        Ok(Self {
            title: NoteTitle::try_new(title).map_err(|error| error.to_string())?,
            content: NoteContent::try_new(content).map_err(|error| error.to_string())?,
        })
    }
}

pub async fn run(arguments: &Arguments, client: &NoteClient) -> anyhow::Result<String> {
    let output = match &arguments.command {
        Command::List { project, number } => client
            .list_notes(ListNotesRequest {
                project_selector: project.to_string(),
                limit_kind: match number {
                    None => NoteListLimitKind::Default as i32,
                    Some(0) => NoteListLimitKind::Unlimited as i32,
                    Some(_) => NoteListLimitKind::AtMost as i32,
                },
                limit: number.unwrap_or_default() as u64,
            })
            .await
            .map_err(crate::rpc_error)
            .map(|result| render_listed(&result))?,
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
            let (title, content) = match (note, title, content) {
                (Some(note), None, None) => (note.title.clone(), note.content.clone()),
                (None, Some(title), Some(content)) => (title.clone(), content.clone()),
                _ => {
                    return Err(anyhow::anyhow!(
                        "Provide either '<title> / <content>' or both --title and --content."
                    ));
                }
            };
            client
                .add_note(AddNoteRequest {
                    project_selector: project.to_string(),
                    title: title.to_string(),
                    content: content.to_string(),
                    why: why.as_ref().map(ToString::to_string),
                    domain: domain.as_ref().map(ToString::to_string),
                    tags: tags.iter().map(ToString::to_string).collect(),
                    sources: sources.iter().map(ToString::to_string).collect(),
                    verified: verified.as_ref().map(ToString::to_string),
                    date: arguments.common.date.map(|date| date.to_string()),
                })
                .await
                .map_err(crate::rpc_error)
                .map(|result| render_added(&result))?
        }
        Command::Remove { project, id } => client
            .remove_note(RemoveNoteRequest {
                project_selector: project.to_string(),
                selector: id.to_string(),
            })
            .await
            .map_err(crate::rpc_error)
            .map(|result| render_removed(&result))?,
        Command::Update { project, id, title } => client
            .update_note(UpdateNoteRequest {
                project_selector: project.to_string(),
                selector: id.to_string(),
                title: NoteTitle::try_new(title.join(" "))
                    .map_err(|error| anyhow::anyhow!(error.to_string()))?
                    .to_string(),
            })
            .await
            .map_err(crate::rpc_error)
            .map(|result| render_updated(&result))?,
    };
    Ok(output)
}

fn render_listed(result: &ListedNotes) -> String {
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

fn render_added(result: &AddedNote) -> String {
    format!("Added {} :: {}\n", result.id, result.title)
}

fn render_removed(result: &RemovedNote) -> String {
    format!("Removed {}\n", result.id)
}

fn render_updated(result: &UpdatedNote) -> String {
    format!("Updated {} :: {}\n", result.id, result.title)
}

#[cfg(test)]
mod tests {
    use pwf_client::v1::{AddedNote, ListedNote, ListedNotes, RemovedNote, UpdatedNote};

    use super::{PositionalNote, render_added, render_listed, render_removed, render_updated};

    fn identifier(number: u32) -> String {
        format!("PWF-NOTE-{number:04}")
    }

    #[test]
    fn typed_results_render_the_existing_note_output_contract() {
        assert_eq!(
            render_added(&AddedNote {
                id: identifier(1),
                title: "remember milk".to_string(),
            }),
            "Added PWF-NOTE-0001 :: remember milk\n"
        );
        assert_eq!(
            render_removed(&RemovedNote { id: identifier(1) }),
            "Removed PWF-NOTE-0001\n"
        );
        assert_eq!(
            render_updated(&UpdatedNote {
                id: identifier(1),
                title: "remember oat milk".to_string(),
            }),
            "Updated PWF-NOTE-0001 :: remember oat milk\n"
        );
    }

    #[test]
    fn listed_results_render_empty_lines_and_hidden_hint() {
        let project = "pwf".to_string();
        assert_eq!(
            render_listed(&ListedNotes {
                project: project.clone(),
                notes: Vec::new(),
                hidden: 0,
            }),
            "No notes for pwf.\n"
        );
        assert_eq!(
            render_listed(&ListedNotes {
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
        let note = "using join / preserve docs/async.md / and later separators"
            .parse::<PositionalNote>()
            .unwrap();

        assert_eq!(note.title.as_ref(), "using join");
        assert_eq!(
            note.content.as_ref(),
            "preserve docs/async.md / and later separators"
        );
        assert!("title/content".parse::<PositionalNote>().is_err());
    }
}
