//! Converts normalized compatibility tokens into typed task leaves.

use clap::Args;

use super::list;

#[derive(Args, Debug)]
pub struct Arguments {
    /// Free-form route words (project + prompt, or a sub-verb).
    pub(crate) words: Vec<String>,
    #[command(flatten)]
    options: list::Options,
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
        options: arguments.options.clone(),
        effort: None,
        priority: None,
        tag: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use pwf_client::pb::TaskStatusFilter;

    use super::*;
    use crate::task::StatusChoice;

    fn arguments(words: &[&str]) -> Arguments {
        Arguments {
            words: words.iter().map(|word| (*word).to_string()).collect(),
            options: list::Options {
                long: true,
                section: Some("Waiting".parse().unwrap()),
                all: false,
                number: Some(3),
                order: None,
                status: Some(StatusChoice::All),
            },
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
        assert!(list.options.long);
        assert_eq!(
            list.options.section.as_ref().map(AsRef::as_ref),
            Some("Waiting")
        );
        assert!(!list.options.all);
        assert_eq!(list.options.number, Some(3));
        assert_eq!(list.options.order, None);
        assert_eq!(list.options.status.unwrap().filter(), TaskStatusFilter::All);
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
