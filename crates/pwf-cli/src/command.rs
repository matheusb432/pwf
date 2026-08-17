//! Defines root parsing and top-level command selection.

use clap::{CommandFactory, Parser, Subcommand};

use crate::{note, project, task};

/// Manages tasks and project notes across configured projects.
#[derive(Parser, Debug)]
#[command(
    name = "pwf",
    version,
    about,
    long_about = None,
    arg_required_else_help = true,
    styles = clap_cargo::style::CLAP_STYLING
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: RootCommand,
}

#[derive(Subcommand, Debug)]
pub enum RootCommand {
    /// Manages registered projects.
    Project(project::Arguments),
    #[command(flatten)]
    Task(task::Command),
    /// Manages a project's study notes.
    Note(note::Arguments),
}

/// Parses post-binary arguments after preserving the accepted normalization pass.
pub fn parse_argv(argv: Vec<String>) -> Result<Cli, clap::Error> {
    let normalized = if argv
        .first()
        .is_some_and(|token| token.eq_ignore_ascii_case("project"))
    {
        argv
    } else {
        crate::preprocess::normalize(argv)
    };
    Cli::try_parse_from(std::iter::once("pwf".to_string()).chain(normalized))
}

/// Renders help scoped to the project command.
pub fn project_help() -> String {
    let mut root = Cli::command();
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
    fn note_update_keeps_accepting_unquoted_title_words() {
        let cli = super::parse_argv(
            ["note", "update", "pwf", "1", "new", "title"]
                .into_iter()
                .map(str::to_string)
                .collect(),
        )
        .unwrap();

        let RootCommand::Note(arguments) = cli.command else {
            panic!("expected note command");
        };
        let note::Command::Update { title, .. } = arguments.command else {
            panic!("expected note update command");
        };
        assert_eq!(title, ["new", "title"]);
    }

    #[test]
    fn project_route_rejects_unsupported_blocked_by_input() {
        let error = super::parse_argv(
            ["pwf", "--blocked-by", "AUX-0001"]
                .into_iter()
                .map(str::to_string)
                .collect(),
        )
        .unwrap_err();

        assert_eq!(error.kind(), clap::error::ErrorKind::UnknownArgument);
    }
}
