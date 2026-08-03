//! Converts normalized compatibility tokens into typed task leaves.

use clap::Args;

use super::{
    list,
    shared::{CommonArguments, SectionChoice, StatusChoice},
};

#[derive(Args, Debug)]
pub struct Arguments {
    /// Free-form route words (project + prompt, or a sub-verb).
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub(crate) words: Vec<String>,
    /// Long form with per-task metadata.
    #[arg(long)]
    pub(crate) long: bool,
    /// Show only this scoped section.
    #[arg(long, value_enum, conflicts_with = "all")]
    pub(crate) section: Option<SectionChoice>,
    /// List everything: every section, every lifecycle status, no task cap.
    /// An explicit `--status` or `-n` overrides the widened default.
    #[arg(long)]
    pub(crate) all: bool,
    /// Cap to N listed tasks, N >= 1 [default: 10, or unlimited under `--all`].
    #[arg(short = 'n', long, value_name = "N", value_parser = clap::builder::RangedU64ValueParser::<usize>::new().range(1..=100_000))]
    pub(crate) number: Option<usize>,
    /// Filter by one lifecycle status, or include every lifecycle status
    /// [default: active, or all under `--all`].
    #[arg(long, value_enum)]
    pub(crate) status: Option<StatusChoice>,
    #[arg(long)]
    pub(crate) prereq: Vec<String>,
    #[command(flatten)]
    pub(crate) common: CommonArguments,
}

pub(crate) enum ResolvedCommand {
    List(list::Arguments),
    RejectUnsupportedTaskCreation,
}

pub(crate) fn resolve(arguments: &Arguments) -> ResolvedCommand {
    let route_words: Vec<&str> = arguments
        .words
        .iter()
        .map(String::as_str)
        .filter(|word| !word.trim().is_empty())
        .collect();

    if route_words.is_empty() {
        return ResolvedCommand::List(list_arguments(arguments, None));
    }

    let verb = route_words[0].to_ascii_lowercase();
    if matches!(verb.as_str(), "add" | "a" | "add-titled" | "at") {
        return ResolvedCommand::RejectUnsupportedTaskCreation;
    }

    if route_words.len() == 1 {
        return ResolvedCommand::List(list_arguments(arguments, Some(verb)));
    }

    ResolvedCommand::RejectUnsupportedTaskCreation
}

fn list_arguments(arguments: &Arguments, project: Option<String>) -> list::Arguments {
    list::Arguments {
        project,
        long: arguments.long,
        section: arguments.section,
        all: arguments.all,
        number: arguments.number,
        effort: None,
        tag: Vec::new(),
        order: None,
        status: arguments.status,
        common: arguments.common.clone(),
        mode: pwf_application::task::ListMode::ProjectRoute,
    }
}

#[cfg(test)]
mod tests {
    use pwf_application::task::{ListMode, StatusFilter};

    use super::*;

    fn arguments(words: &[&str]) -> Arguments {
        Arguments {
            words: words.iter().map(|word| (*word).to_string()).collect(),
            long: true,
            section: Some(SectionChoice::Future),
            all: true,
            number: Some(3),
            status: Some(StatusChoice::All),
            prereq: Vec::new(),
            common: CommonArguments::default(),
        }
    }

    #[test]
    fn project_route_preserves_list_options() {
        let ResolvedCommand::List(list) = resolve(&arguments(&["pwf"])) else {
            panic!("expected routed list");
        };

        assert_eq!(list.project.as_deref(), Some("pwf"));
        assert!(list.long);
        assert!(matches!(list.section, Some(SectionChoice::Future)));
        assert!(list.all);
        assert_eq!(list.number, Some(3));
        assert_eq!(list.mode, ListMode::ProjectRoute);
        assert_eq!(
            list.status.expect("explicit status").filter(),
            StatusFilter::All
        );
    }

    #[test]
    fn create_and_multiword_routes_reach_the_rejection_owner() {
        let ResolvedCommand::RejectUnsupportedTaskCreation = resolve(&arguments(&["add"])) else {
            panic!("expected rejected create");
        };

        let ResolvedCommand::RejectUnsupportedTaskCreation =
            resolve(&arguments(&["pwf", "build", "it"]))
        else {
            panic!("expected rejected create");
        };
    }
}
