//! Defines root parsing and top-level command selection.

use clap::{Parser, Subcommand};

use crate::{note, pending_work, project};

/// Manages pending work and project notes across configured repositories.
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
    PendingWork(pending_work::Command),
    /// Study notes: `pwf note [list|add|update|remove] <project>`.
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
    let mut help = Cli::try_parse_from(["pwf", "project", "--help"])
        .expect_err("project help exits clap parsing")
        .render()
        .to_string();
    if help.ends_with('\n') {
        help.pop();
    }
    help
}
