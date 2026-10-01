//! Defines root parsing and top-level command selection.

use std::fmt;

use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand, error::ErrorKind};

use crate::{data, note, project, task};

pub const PWF_COMMAND: CommandName = CommandName {
    name: "pwf",
    parent: None,
};
pub const DOCTOR_COMMAND: CommandName = PWF_COMMAND.subcommand("doctor");
pub(crate) const DATA_COMMAND: CommandName = PWF_COMMAND.subcommand("data");
pub(crate) const SERVER_COMMAND: CommandName = PWF_COMMAND.subcommand("server");
pub(crate) const SERVER_INSTALL_COMMAND: CommandName = SERVER_COMMAND.subcommand("install");
pub(crate) const SERVER_START_COMMAND: CommandName = SERVER_COMMAND.subcommand("start");
pub(crate) const SERVER_RESTART_COMMAND: CommandName = SERVER_COMMAND.subcommand("restart");
pub(crate) const SERVER_STATUS_COMMAND: CommandName = SERVER_COMMAND.subcommand("status");
pub const SERVER_BINARY_COMMAND: CommandName = CommandName {
    name: "pwf-server",
    parent: None,
};
pub const SERVER_DOCTOR_COMMAND: CommandName = SERVER_BINARY_COMMAND.subcommand("doctor");

/// Binds a clap command name and its full invocation for recovery messages.
pub struct CommandName {
    name: &'static str,
    parent: Option<&'static Self>,
}

impl CommandName {
    const fn subcommand(&'static self, name: &'static str) -> Self {
        Self {
            name,
            parent: Some(self),
        }
    }

    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }
}

impl fmt::Display for CommandName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(parent) = self.parent {
            write!(formatter, "{parent} ")?;
        }
        formatter.write_str(self.name)
    }
}

/// Manages tasks and project notes across configured projects.
#[derive(Debug)]
pub struct Cli {
    pub command: RootCommand,
    pub(crate) context: clap::Command,
}

#[derive(Debug)]
pub enum RootCommand {
    Tui(pwf_tui::Arguments),
    Doctor(crate::doctor::Arguments),
    Server(crate::server::Arguments),
    Data(data::Arguments),
    Project(project::Arguments),
    Task(task::Command),
    Note(note::Arguments),
}

#[derive(Parser, Debug)]
#[command(
    name = PWF_COMMAND.name(),
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
    /// Browse, create, and manage tasks and notes in an interactive terminal.
    Tui(pwf_tui::Arguments),
    /// Check local server health and show recovery actions without changing state.
    #[command(name = DOCTOR_COMMAND.name())]
    Doctor(crate::doctor::Arguments),
    /// Manage the local background server and login startup.
    #[command(name = SERVER_COMMAND.name())]
    Server(crate::server::Arguments),
    #[command(name = DATA_COMMAND.name(), about = "Exports or imports pwf's portable data for cross-machine handoff.")]
    Data(data::Arguments),
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
    let context = selected_command(&command, &matches).clone();
    let command = match parsed.command {
        ParsedRootCommand::Tui(arguments) => RootCommand::Tui(arguments),
        ParsedRootCommand::Doctor(arguments) => RootCommand::Doctor(arguments),
        ParsedRootCommand::Server(arguments) => RootCommand::Server(arguments),
        ParsedRootCommand::Data(arguments) => RootCommand::Data(arguments),
        ParsedRootCommand::Project(arguments) => RootCommand::Project(arguments),
        ParsedRootCommand::Task(command) => RootCommand::Task(command),
        ParsedRootCommand::Note(arguments) => return parse_note_arguments(arguments, context),
        ParsedRootCommand::Shorthand(arguments) => return parse_task_or_route(arguments),
    };
    Ok(Cli { command, context })
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

fn parse_note_arguments(
    arguments: ParsedNoteArguments,
    context: clap::Command,
) -> Result<Cli, clap::Error> {
    match arguments.command {
        ParsedNoteCommand::Explicit(command) => Ok(Cli {
            command: RootCommand::Note(note::Arguments { command }),
            context,
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
            if let Some((project, options)) = arguments.split_first()
                && project.parse::<pwf_models::project::ProjectId>().is_ok()
            {
                let argv = ["task", "list", "--project", project]
                    .into_iter()
                    .chain(options.iter().map(String::as_str))
                    .map(str::to_string)
                    .collect();
                return parse_normalized_argv(argv);
            }
            let mut root = ParsedCli::command()
                .allow_external_subcommands(false)
                .external_subcommand_value_parser(None);
            match root.try_get_matches_from_mut(std::iter::once("pwf".to_string()).chain(arguments))
            {
                Err(error) => Err(error),
                Ok(_) => Err(root.error(ErrorKind::InvalidSubcommand, "unrecognized subcommand")),
            }
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
    use clap::CommandFactory as _;
    use pwf_models::note::NOTE_TITLE_CHARACTER_LIMIT;

    use super::RootCommand;
    use crate::note;

    #[test]
    fn recovery_commands_resolve_to_their_clap_invocations() {
        let mut root = super::ParsedCli::command();
        root.build();
        for command in [
            &super::DOCTOR_COMMAND,
            &super::SERVER_INSTALL_COMMAND,
            &super::SERVER_START_COMMAND,
            &super::SERVER_RESTART_COMMAND,
            &super::SERVER_STATUS_COMMAND,
        ] {
            let invocation = command.to_string();
            let mut selected = &root;
            for name in invocation.split_whitespace().skip(1) {
                selected = selected.find_subcommand(name).unwrap();
            }
            assert_eq!(selected.get_bin_name(), Some(invocation.as_str()));
        }
    }

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
