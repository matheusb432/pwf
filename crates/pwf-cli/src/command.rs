//! Defines root parsing and top-level command selection.

use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand, error::ErrorKind};

use crate::{note, project, task};

/// Manages tasks and project notes across configured projects.
#[derive(Debug)]
pub struct Cli {
    pub command: RootCommand,
}

#[derive(Debug)]
pub enum RootCommand {
    Server(crate::server::Arguments),
    Project(project::Arguments),
    Task(task::Command),
    Note(note::Arguments),
}

#[derive(Parser, Debug)]
#[command(
    name = "pwf",
    version,
    about,
    long_about = None,
    arg_required_else_help = true,
    styles = clap_cargo::style::CLAP_STYLING
)]
struct ParsedCli {
    #[command(subcommand)]
    command: ParsedRootCommand,
}

#[derive(Subcommand, Debug)]
enum ParsedRootCommand {
    /// Manage the local background server and login startup.
    Server(crate::server::Arguments),
    /// Manages registered projects.
    Project(project::Arguments),
    #[command(flatten)]
    Task(task::Command),
    /// Manages a project's study notes.
    Note(ParsedNoteArguments),
    #[command(external_subcommand)]
    Shorthand(Vec<String>),
}

#[derive(Args, Debug)]
struct ParsedNoteArguments {
    #[command(subcommand)]
    command: ParsedNoteCommand,
}

#[derive(Subcommand, Debug)]
enum ParsedNoteCommand {
    #[command(flatten)]
    Explicit(note::Command),
    #[command(external_subcommand)]
    ImplicitList(Vec<String>),
}

/// Parses post-binary arguments, including the nounless task and list shorthands.
pub fn parse_argv(argv: Vec<String>) -> Result<Cli, clap::Error> {
    parse_normalized_argv(argv)
}

fn parse_normalized_argv(argv: Vec<String>) -> Result<Cli, clap::Error> {
    let mut command = ParsedCli::command();
    let matches =
        command.try_get_matches_from_mut(std::iter::once("pwf".to_string()).chain(argv))?;
    let parsed = ParsedCli::from_arg_matches(&matches).map_err(|error| {
        let mut selected_command = selected_command(&command, &matches).clone();
        error.format(&mut selected_command)
    })?;
    match parsed.command {
        ParsedRootCommand::Server(arguments) => Ok(Cli {
            command: RootCommand::Server(arguments),
        }),
        ParsedRootCommand::Project(arguments) => Ok(Cli {
            command: RootCommand::Project(arguments),
        }),
        ParsedRootCommand::Task(command) => Ok(Cli {
            command: RootCommand::Task(command),
        }),
        ParsedRootCommand::Note(arguments) => parse_note_arguments(arguments),
        ParsedRootCommand::Shorthand(arguments) => parse_task_or_route(arguments),
    }
}

fn selected_command<'a>(
    command: &'a clap::Command,
    matches: &clap::ArgMatches,
) -> &'a clap::Command {
    let Some((subcommand_name, subcommand_matches)) = matches.subcommand() else {
        return command;
    };
    let Some(subcommand) = command.find_subcommand(subcommand_name) else {
        return command;
    };
    selected_command(subcommand, subcommand_matches)
}

fn parse_note_arguments(arguments: ParsedNoteArguments) -> Result<Cli, clap::Error> {
    match arguments.command {
        ParsedNoteCommand::Explicit(command) => Ok(Cli {
            command: RootCommand::Note(note::Arguments { command }),
        }),
        ParsedNoteCommand::ImplicitList(arguments) => parse_note_list(arguments),
    }
}

fn parse_note_list(arguments: Vec<String>) -> Result<Cli, clap::Error> {
    if matches!(
        arguments.first().map(String::as_str),
        Some("get" | "update")
    ) {
        return Err(invalid_note_subcommand(arguments));
    }

    let argv = ["note".to_string(), "list".to_string()]
        .into_iter()
        .chain(arguments)
        .collect();
    parse_normalized_argv(argv)
}

fn invalid_note_subcommand(arguments: Vec<String>) -> clap::Error {
    let argv = std::iter::once("pwf".to_string()).chain(arguments);
    let result = note::Arguments::augment_args(clap::Command::new("pwf").bin_name("pwf note"))
        .try_get_matches_from(argv);
    match result {
        Err(error) => error,
        Ok(_) => clap::Error::raw(ErrorKind::InvalidSubcommand, "invalid note subcommand"),
    }
}

fn parse_task_or_route(arguments: Vec<String>) -> Result<Cli, clap::Error> {
    let task_argv = std::iter::once("task".to_string())
        .chain(arguments.iter().cloned())
        .collect();
    match parse_normalized_argv(task_argv) {
        Err(error) if error.kind() == ErrorKind::InvalidSubcommand => {
            let route_argv = std::iter::once("route".to_string())
                .chain(arguments)
                .collect();
            parse_normalized_argv(route_argv)
        }
        result => result,
    }
}

/// Renders help scoped to the project command.
#[must_use]
pub fn project_help() -> String {
    let mut root = ParsedCli::command();
    let mut help = match root.find_subcommand("project") {
        Some(project) => {
            let mut project = project.clone().bin_name("pwf project");
            project.render_help().to_string()
        }
        None => root.render_help().to_string(),
    };
    if help.ends_with('\n') {
        help.pop();
    }
    help
}

#[cfg(test)]
mod tests {
    use pwf_models::note::NOTE_TITLE_CHARACTER_LIMIT;

    use super::RootCommand;
    use crate::note;

    #[test]
    fn note_title_validation_happens_during_argument_parsing() {
        let title = "e".repeat(NOTE_TITLE_CHARACTER_LIMIT + 1);

        let error = super::parse_argv(vec![
            "note".to_string(),
            "add".to_string(),
            "pwf".to_string(),
            "--title".to_string(),
            title,
            "--content".to_string(),
            "content".to_string(),
        ])
        .unwrap_err();

        assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation);
    }

    #[test]
    fn note_edit_keeps_accepting_unquoted_title_words() {
        let cli = super::parse_argv(
            ["note", "edit", "foo", "1", "new", "title"]
                .into_iter()
                .map(str::to_string)
                .collect(),
        )
        .unwrap();

        let arguments = match cli.command {
            RootCommand::Note(arguments) => Some(arguments),
            _ => None,
        };
        assert!(arguments.is_some());
        assert!(matches!(arguments.unwrap().command, note::Command::Edit(_)));
    }

    #[test]
    fn note_update_is_not_an_alias_or_an_implicit_project_listing() {
        let error = super::parse_argv(
            ["note", "update", "foo", "1", "new title"]
                .into_iter()
                .map(str::to_string)
                .collect(),
        )
        .unwrap_err();

        assert_eq!(error.kind(), clap::error::ErrorKind::InvalidSubcommand);
    }

    #[test]
    fn project_route_rejects_unsupported_blocked_by_input() {
        let error = super::parse_argv(
            ["foo", "--blocked-by", "AUX-0001"]
                .into_iter()
                .map(str::to_string)
                .collect(),
        )
        .unwrap_err();

        assert_eq!(error.kind(), clap::error::ErrorKind::UnknownArgument);
    }

    #[test]
    fn root_task_commands_keep_priority_with_its_value() {
        for arguments in [
            ["add", "foo", "ship it", "--priority", "highest"].as_slice(),
            ["edit", "foo1", "--priority", "high"].as_slice(),
            ["list", "--priority", "medium"].as_slice(),
        ] {
            assert!(
                super::parse_argv(arguments.iter().map(ToString::to_string).collect()).is_ok(),
                "failed to parse {arguments:?}"
            );
        }
    }

    #[test]
    fn session_accepts_one_comma_separated_task_id_value() {
        for arguments in [
            ["session", "foo23,foo15"].as_slice(),
            ["session", "--id", "foo23,foo15"].as_slice(),
        ] {
            assert!(super::parse_argv(arguments.iter().map(ToString::to_string).collect()).is_ok());
        }
    }

    #[test]
    fn session_rejects_duplicate_mixed_project_and_oversized_id_lists() {
        for ids in ["foo1,foo1", "foo1,bar2", "foo1,foo2,foo3,foo4,foo5,foo6"] {
            let error = super::parse_argv(vec![
                "session".to_string(),
                "--id".to_string(),
                ids.to_string(),
            ])
            .unwrap_err();

            assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation);
        }
    }

    #[test]
    fn session_rejects_space_separated_and_repeated_id_arguments() {
        for arguments in [
            vec!["session", "foo1", "foo2"],
            vec!["session", "--id", "foo1", "--id", "foo2"],
        ] {
            assert!(
                super::parse_argv(arguments.into_iter().map(str::to_string).collect()).is_err()
            );
        }
    }
}
