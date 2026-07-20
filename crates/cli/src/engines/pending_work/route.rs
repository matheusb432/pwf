//! Converts normalized compatibility tokens into typed pending-work leaves.

use clap::Args;

use super::{
    common::{AgentChoice, CommonArguments, Identifier, StatusChoice},
    list, verify,
};

#[derive(Args, Debug)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "clap mirrors independent command-line switches"
)]
pub struct Arguments {
    /// Free-form route words (project + prompt, or a sub-verb).
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub(crate) words: Vec<String>,
    /// Long form with per-item metadata.
    #[arg(long)]
    pub(crate) long: bool,
    /// Show only `## Future` items.
    #[arg(long, conflicts_with_all = ["human", "all"])]
    pub(crate) future: bool,
    /// Show only `## Human` items.
    #[arg(long, conflicts_with_all = ["future", "all"])]
    pub(crate) human: bool,
    /// Include every list section.
    #[arg(long, conflicts_with_all = ["human", "future"])]
    pub(crate) all: bool,
    /// Cap to N listed items.
    #[arg(short = 'n', long, value_name = "N")]
    pub(crate) number: Option<usize>,
    /// Filter by one lifecycle status, or include every lifecycle status.
    #[arg(long, value_enum, default_value_t = StatusChoice::Active)]
    pub(crate) status: StatusChoice,
    #[arg(long)]
    pub(crate) prereq: Vec<String>,
    #[command(flatten)]
    pub(crate) common: CommonArguments,
}

pub(crate) enum ResolvedCommand {
    List(list::Arguments),
    Verify(verify::Arguments),
}

pub(crate) fn resolve(arguments: &Arguments) -> ResolvedCommand {
    let route_words: Vec<&str> = arguments
        .words
        .iter()
        .map(String::as_str)
        .filter(|word| !word.trim().is_empty())
        .collect();

    if route_words.is_empty() {
        return ResolvedCommand::List(list_arguments(arguments, None, list::Compatibility::List));
    }

    let verb = route_words[0].to_ascii_lowercase();
    if matches!(verb.as_str(), "add" | "a" | "add-titled" | "at") {
        return ResolvedCommand::List(list_arguments(
            arguments,
            None,
            list::Compatibility::RejectCreate,
        ));
    }

    if verb == "verify" || verb == "v" {
        return ResolvedCommand::Verify(verify::Arguments {
            identifier: Identifier::from_positional(route_words.get(1).map(|id| (*id).to_string())),
            agent: AgentChoice::Claude,
            model: None,
            common: arguments.common.clone(),
        });
    }

    if route_words.len() == 1 {
        return ResolvedCommand::List(list_arguments(
            arguments,
            Some(verb),
            list::Compatibility::List,
        ));
    }

    ResolvedCommand::List(list_arguments(
        arguments,
        Some(verb),
        list::Compatibility::RejectCreateAfterProjectResolution,
    ))
}

fn list_arguments(
    arguments: &Arguments,
    project: Option<String>,
    compatibility: list::Compatibility,
) -> list::Arguments {
    list::Arguments {
        project,
        long: arguments.long,
        future: arguments.future,
        human: arguments.human,
        all: arguments.all,
        number: arguments.number,
        effort: None,
        tag: Vec::new(),
        order: vec!["project-id".to_string()],
        status: arguments.status,
        common: arguments.common.clone(),
        compatibility,
    }
}
