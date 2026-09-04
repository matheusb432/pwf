//! Converts normalized compatibility tokens into typed task leaves.

use clap::Args;
use pwf_client::pb::OrderSpec;
use pwf_models::task::TaskSection;

use super::{StatusChoice, list};

#[derive(Args, Debug)]
pub struct Arguments {
    /// Free-form route words (project + prompt, or a sub-verb).
    pub(crate) words: Vec<String>,
    /// Long form with per-task metadata.
    #[arg(long)]
    pub(crate) long: bool,
    /// Show only the section whose `##` header matches this text, ignoring case.
    #[arg(long, value_name = "HEADER", conflicts_with = "all")]
    pub(crate) section: Option<TaskSection>,
    /// List everything: every section, every lifecycle status, no task cap.
    /// An explicit `--status` or `-n` overrides the widened default.
    #[arg(long)]
    pub(crate) all: bool,
    /// Cap to N listed tasks, N >= 1 [default: 10, or unlimited under `--all`].
    #[arg(short = 'n', long, value_name = "N", value_parser = clap::builder::RangedU64ValueParser::<usize>::new().range(1..=100_000))]
    pub(crate) number: Option<usize>,
    /// Sort key field[:direction], overriding `default_sort_order` in config.toml.
    #[arg(short = 'o', long, value_name = "FIELD[:DIR]", value_parser = list::parse_order)]
    pub(crate) order: Option<OrderSpec>,
    /// Filter by one lifecycle status, or include every lifecycle status
    /// [default: active, or all under `--all`].
    #[arg(long, value_enum)]
    pub(crate) status: Option<StatusChoice>,
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
        let Ok(project) = verb.parse() else {
            return ResolvedCommand::RejectUnsupportedTaskCreation;
        };
        return ResolvedCommand::List(list_arguments(arguments, Some(project)));
    }

    ResolvedCommand::RejectUnsupportedTaskCreation
}

fn list_arguments(
    arguments: &Arguments,
    project: Option<pwf_models::project::ProjectSelector>,
) -> list::Arguments {
    list::Arguments {
        project,
        long: arguments.long,
        section: arguments.section.clone(),
        all: arguments.all,
        number: arguments.number,
        effort: None,
        priority: None,
        tag: Vec::new(),
        order: arguments.order,
        status: arguments.status,
    }
}

#[cfg(test)]
mod tests {
    use pwf_client::pb::TaskStatusFilter;

    use super::*;

    fn arguments(words: &[&str]) -> Arguments {
        Arguments {
            words: words.iter().map(|word| (*word).to_string()).collect(),
            long: true,
            section: Some("Waiting".parse().unwrap()),
            all: false,
            number: Some(3),
            order: None,
            status: Some(StatusChoice::All),
        }
    }

    #[test]
    fn project_route_preserves_list_options() {
        let list = match resolve(&arguments(&["foo"])) {
            ResolvedCommand::List(list) => Some(list),
            ResolvedCommand::RejectUnsupportedTaskCreation => None,
        };
        assert!(list.is_some());
        let list = list.unwrap();

        assert_eq!(list.project.as_ref().map(AsRef::as_ref), Some("foo"));
        assert!(list.long);
        assert_eq!(list.section.as_ref().map(AsRef::as_ref), Some("Waiting"));
        assert!(!list.all);
        assert_eq!(list.number, Some(3));
        assert_eq!(list.order, None);
        assert_eq!(list.status.unwrap().filter(), TaskStatusFilter::All);
    }

    #[test]
    fn create_and_multiword_routes_reach_the_rejection_owner() {
        assert!(matches!(
            resolve(&arguments(&["add"])),
            ResolvedCommand::RejectUnsupportedTaskCreation
        ));
        assert!(matches!(
            resolve(&arguments(&["foo", "build", "it"])),
            ResolvedCommand::RejectUnsupportedTaskCreation
        ));
    }
}
