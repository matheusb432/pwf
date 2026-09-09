//! Owns project-note parsing, request mapping, and rendering.

use std::{fmt::Write, str::FromStr};

use anstyle::AnsiColor;
use clap::{ArgGroup, Args, Subcommand};
use pwf_client::{
    confirmation::ConfirmedRequestError,
    note::NoteClient,
    pb::{
        AddNoteRequest, AddNoteResponse, DeleteNoteStart, DeletedNote, ListNotesRequest,
        ListNotesResponse, NoteListLimitKind, UpdateNoteRequest, UpdateNoteResponse,
        delete_note_result,
    },
};
use pwf_models::{
    AppDate,
    note::{
        NoteContent, NoteDomain, NoteSelector, NoteSource, NoteTag, NoteTitle, NoteVerification,
    },
    project::ProjectSelector,
    settings::NoteStatusColors,
};

use crate::{
    confirmation::{CliConfirmationClient, prompt_error},
    console::Console,
    edit::{string_collection_edit, string_field_edit},
    render::{render_confirmation, render_summary, rgb_color},
};

#[derive(Args, Debug)]
pub struct Arguments {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Subcommand, Debug)]
pub(crate) enum Command {
    /// List a project's notes
    #[command(alias = "ls")]
    List(ListArguments),
    /// Add a study note from `<title> / <content>` or explicit fields
    Add(Box<AddArguments>),
    /// Delete a note and strip its index link
    Remove(RemoveArguments),
    /// Edit selected note fields while preserving every omitted field
    Edit(Box<EditArguments>),
}

#[derive(Args, Debug)]
pub(crate) struct ListArguments {
    /// Managed project name or id
    #[arg(value_name = "PROJECT")]
    project: ProjectSelector,
    /// Cap to N listed notes (default 10)
    #[arg(short = 'n', long, value_name = "N")]
    number: Option<usize>,
}

#[derive(Args, Debug)]
pub(crate) struct AddArguments {
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
    /// Subject classification
    #[arg(long)]
    domain: Option<NoteDomain>,
    /// Discovery tag; repeat for several
    #[arg(long = "tag", value_name = "TAG")]
    tags: Vec<NoteTag>,
    /// Supporting source or evidence; repeat for several
    #[arg(long = "source", value_name = "SOURCE")]
    sources: Vec<NoteSource>,
    /// Verification date or marker
    #[arg(long)]
    verified: Option<NoteVerification>,
    /// Date stamp (YYYY-MM-DD); defaults to today
    #[arg(long)]
    date: Option<AppDate>,
}

#[derive(Args, Debug)]
pub(crate) struct RemoveArguments {
    /// Managed project name or id
    #[arg(value_name = "PROJECT")]
    project: ProjectSelector,
    /// Note id: full `PWF-NOTE-0001`, `NOTE-0001`, or a bare `1`
    #[arg(value_name = "ID")]
    id: NoteSelector,
    /// Skip the removal confirmation (assume yes)
    #[arg(long = "yes", short = 'y')]
    assume_yes: bool,
}

#[derive(Args, Debug)]
#[command(group(
    ArgGroup::new("edit")
        .required(true)
        .multiple(true)
        .args([
            "shorthand_title",
            "title",
            "content",
            "domain",
            "remove_domain",
            "add_tag",
            "remove_tags",
            "add_source",
            "remove_sources",
            "verified",
            "remove_verified",
        ])
))]
pub(crate) struct EditArguments {
    /// Managed project name or id
    #[arg(value_name = "PROJECT")]
    project: ProjectSelector,
    /// Note id: full `PWF-NOTE-0001`, `NOTE-0001`, or a bare `1`
    #[arg(value_name = "ID")]
    id: NoteSelector,
    /// Replacement title words; shorthand alternative to `--title`
    #[arg(value_name = "TITLE", num_args = 1.., conflicts_with = "title")]
    shorthand_title: Vec<String>,
    /// Replace the title
    #[arg(long, conflicts_with = "shorthand_title")]
    title: Option<NoteTitle>,
    /// Replace the Markdown content
    #[arg(long)]
    content: Option<NoteContent>,
    #[command(flatten)]
    domain: DomainEdits,
    #[command(flatten)]
    tags: TagEdits,
    #[command(flatten)]
    sources: SourceEdits,
    #[command(flatten)]
    verification: VerificationEdits,
}

#[derive(Args, Debug)]
struct DomainEdits {
    /// Replace the subject classification
    #[arg(long, conflicts_with = "remove_domain")]
    domain: Option<NoteDomain>,
    /// Remove the subject classification
    #[arg(long, conflicts_with = "domain")]
    remove_domain: bool,
}

#[derive(Args, Debug)]
struct TagEdits {
    /// Append a discovery tag; repeat for several
    #[arg(long, value_name = "TAG")]
    add_tag: Vec<NoteTag>,
    /// Remove every tag before applying `--add-tag` values
    #[arg(long)]
    remove_tags: bool,
}

#[derive(Args, Debug)]
struct SourceEdits {
    /// Append a supporting source; repeat for several
    #[arg(long, value_name = "SOURCE")]
    add_source: Vec<NoteSource>,
    /// Remove every source before applying `--add-source` values
    #[arg(long)]
    remove_sources: bool,
}

#[derive(Args, Debug)]
struct VerificationEdits {
    /// Replace the verification date or marker
    #[arg(long, conflicts_with = "remove_verified")]
    verified: Option<NoteVerification>,
    /// Remove the verification date or marker
    #[arg(long, conflicts_with = "verified")]
    remove_verified: bool,
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

pub async fn run(
    arguments: &Arguments,
    console: Console,
    colors: NoteStatusColors,
    client: &NoteClient,
) -> anyhow::Result<String> {
    match &arguments.command {
        Command::List(arguments) => list(arguments, console, colors, client).await,
        Command::Add(arguments) => add(arguments, console, client).await,
        Command::Remove(arguments) => remove(arguments, console, client).await,
        Command::Edit(arguments) => edit(arguments, console, client).await,
    }
}

async fn list(
    arguments: &ListArguments,
    console: Console,
    colors: NoteStatusColors,
    client: &NoteClient,
) -> anyhow::Result<String> {
    client
        .list_notes(ListNotesRequest {
            project_selector: arguments.project.to_string(),
            limit_kind: match arguments.number {
                None => NoteListLimitKind::Default as i32,
                Some(0) => NoteListLimitKind::Unlimited as i32,
                Some(_) => NoteListLimitKind::AtMost as i32,
            },
            limit: arguments.number.unwrap_or_default() as u64,
        })
        .await
        .map_err(crate::rpc_error)
        .map(|result| render_listed(&result, colors, console.color()))
}

async fn add(
    arguments: &AddArguments,
    console: Console,
    client: &NoteClient,
) -> anyhow::Result<String> {
    let (title, content) = match (&arguments.note, &arguments.title, &arguments.content) {
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
            project_selector: arguments.project.to_string(),
            title: title.to_string(),
            content: content.to_string(),
            domain: arguments.domain.as_ref().map(ToString::to_string),
            tags: arguments.tags.iter().map(ToString::to_string).collect(),
            sources: arguments.sources.iter().map(ToString::to_string).collect(),
            verified: arguments.verified.as_ref().map(ToString::to_string),
            date: arguments.date.map(|date| date.to_string()),
        })
        .await
        .map_err(crate::rpc_error)
        .map(|result| render_added(&result, console.color()))
}

async fn remove(
    arguments: &RemoveArguments,
    console: Console,
    client: &NoteClient,
) -> anyhow::Result<String> {
    let confirmation_mode = console.confirmation_mode(arguments.assume_yes)?;
    let confirmation_client = CliConfirmationClient::new(console, confirmation_mode);
    let outcome = match client
        .delete_note(
            DeleteNoteStart {
                project_selector: arguments.project.to_string(),
                selector: arguments.id.to_string(),
            },
            confirmation_client,
        )
        .await
    {
        Ok(outcome) => outcome,
        Err(ConfirmedRequestError::Operation(error)) => {
            return Err(anyhow::anyhow!(error.message().to_string()));
        }
        Err(ConfirmedRequestError::Prompt(source)) => {
            return Err(prompt_error("note removal", source));
        }
    };
    match outcome.outcome.as_ref() {
        Some(delete_note_result::Outcome::Deleted(note)) => {
            Ok(render_removed(note, console.color()))
        }
        Some(delete_note_result::Outcome::Aborted(note)) => {
            Ok(format!("# remove {}: aborted\nnothing deleted.\n", note.id))
        }
        None => Err(anyhow::anyhow!(
            "pwf-server returned an invalid note removal outcome"
        )),
    }
}

async fn edit(
    arguments: &EditArguments,
    console: Console,
    client: &NoteClient,
) -> anyhow::Result<String> {
    let title = if arguments.shorthand_title.is_empty() {
        arguments.title.clone()
    } else {
        Some(
            NoteTitle::try_new(arguments.shorthand_title.join(" "))
                .map_err(|error| anyhow::anyhow!(error.to_string()))?,
        )
    };
    client
        .update_note(UpdateNoteRequest {
            project_selector: arguments.project.to_string(),
            selector: arguments.id.to_string(),
            title: title.map(|title| title.to_string()),
            content: arguments.content.as_ref().map(ToString::to_string),
            domain: string_field_edit(
                arguments.domain.domain.as_ref().map(ToString::to_string),
                arguments.domain.remove_domain,
            ),
            tags: string_collection_edit(
                arguments
                    .tags
                    .add_tag
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
                arguments.tags.remove_tags,
            ),
            sources: string_collection_edit(
                arguments
                    .sources
                    .add_source
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
                arguments.sources.remove_sources,
            ),
            verified: string_field_edit(
                arguments
                    .verification
                    .verified
                    .as_ref()
                    .map(ToString::to_string),
                arguments.verification.remove_verified,
            ),
        })
        .await
        .map_err(crate::rpc_error)
        .map(|result| render_edited(&result, console.color()))
}

fn render_listed(result: &ListNotesResponse, colors: NoteStatusColors, color_on: bool) -> String {
    if result.notes.is_empty() {
        return format!("No notes for {}.\n", result.project);
    }
    let mut output = result
        .notes
        .iter()
        .map(|note| {
            let color = if note.is_verified {
                colors.verified()
            } else {
                colors.active()
            };
            render_summary(&note.id, &note.title, rgb_color(color), color_on)
        })
        .collect::<Vec<_>>()
        .join("\n");
    if result.hidden > 0 {
        let _ = write!(
            output,
            "\n... and {} more; run 'pwf note <proj> ls -n 0' to show all",
            result.hidden
        );
    }
    output
}

fn render_added(result: &AddNoteResponse, color_on: bool) -> String {
    render_note_mutation(
        "Added pwf note",
        AnsiColor::Green,
        &result.id,
        &result.project,
        &result.title,
        color_on,
    )
}

fn render_edited(result: &UpdateNoteResponse, color_on: bool) -> String {
    render_note_mutation(
        "Edited pwf note",
        AnsiColor::Blue,
        &result.id,
        &result.project,
        &result.title,
        color_on,
    )
}

fn render_removed(result: &DeletedNote, color_on: bool) -> String {
    render_note_mutation(
        "Removed pwf note",
        AnsiColor::Red,
        &result.id,
        &result.project,
        &result.title,
        color_on,
    )
}

fn render_note_mutation(
    label: &str,
    color: AnsiColor,
    id: &str,
    project: &str,
    title: &str,
    color_on: bool,
) -> String {
    render_confirmation(
        label,
        color,
        id,
        &format!("{project} :: {title}"),
        &[],
        color_on,
    )
}

#[cfg(test)]
mod tests {
    use pwf_client::pb::{
        AddNoteResponse, DeletedNote, ListNotesResponse, ListedNote, UpdateNoteResponse,
    };

    use super::{
        NoteStatusColors, PositionalNote, render_added, render_edited, render_listed,
        render_removed,
    };

    fn identifier(number: u32) -> String {
        format!("FOO-NOTE-{number:04}")
    }

    #[test]
    fn mutations_use_the_shared_task_style() {
        assert_eq!(
            render_added(
                &AddNoteResponse {
                    id: identifier(1),
                    title: "remember milk".to_string(),
                    project: "foo".to_string(),
                },
                false,
            ),
            "Added pwf note: **FOO-NOTE-0001 foo :: remember milk**\n"
        );
        assert_eq!(
            render_edited(
                &UpdateNoteResponse {
                    id: identifier(1),
                    title: "remember oat milk".to_string(),
                    project: "foo".to_string(),
                },
                false,
            ),
            "Edited pwf note: **FOO-NOTE-0001 foo :: remember oat milk**\n"
        );
        assert_eq!(
            render_removed(
                &DeletedNote {
                    id: identifier(1),
                    title: "remember oat milk".to_string(),
                    project: "foo".to_string(),
                },
                false,
            ),
            "Removed pwf note: **FOO-NOTE-0001 foo :: remember oat milk**\n"
        );
    }

    #[test]
    fn listed_results_render_empty_lines_and_hidden_hint() {
        assert_eq!(
            render_listed(
                &ListNotesResponse {
                    project: "foo".to_string(),
                    notes: Vec::new(),
                    hidden: 0,
                },
                NoteStatusColors::default(),
                false,
            ),
            "No notes for foo.\n"
        );
        assert_eq!(
            render_listed(
                &ListNotesResponse {
                    project: "foo".to_string(),
                    notes: vec![
                        ListedNote {
                            is_verified: false,
                            id: identifier(2),
                            title: "second".to_string(),
                        },
                        ListedNote {
                            is_verified: false,
                            id: identifier(1),
                            title: "first".to_string(),
                        },
                    ],
                    hidden: 3,
                },
                NoteStatusColors::default(),
                false,
            ),
            "FOO-NOTE-0002 :: second\nFOO-NOTE-0001 :: first\n... and 3 more; run 'pwf note <proj> ls -n 0' to show all"
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
