//! Converts normalized compatibility tokens into typed pending-work leaves.

use clap::Args;
use pwf_application::pending_work::{
    ProjectRegistry,
    reject_pending_work_create::{self, RejectPendingWorkCreate},
};

use super::{
    common::{
        AgentChoice, CommonArguments, Identifier, PendingWorkError, SectionChoice, StatusChoice,
    },
    list, verify,
};

#[derive(Args, Debug)]
pub struct Arguments {
    /// Free-form route words (project + prompt, or a sub-verb).
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub(crate) words: Vec<String>,
    /// Long form with per-item metadata.
    #[arg(long)]
    pub(crate) long: bool,
    /// Show only this scoped section.
    #[arg(long, value_enum, conflicts_with = "all")]
    pub(crate) section: Option<SectionChoice>,
    /// List everything: every section, every lifecycle status, no item cap.
    /// An explicit `--status` or `-n` overrides the widened default.
    #[arg(long)]
    pub(crate) all: bool,
    /// Cap to N listed items, N >= 1 [default: 10, or unlimited under `--all`].
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
    Verify(verify::Arguments),
    RejectCreate(RejectCreateArguments),
}

pub(crate) struct RejectCreateArguments {
    project_identifier: Option<String>,
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
        return ResolvedCommand::RejectCreate(RejectCreateArguments {
            project_identifier: None,
        });
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
        return ResolvedCommand::List(list_arguments(arguments, Some(verb)));
    }

    ResolvedCommand::RejectCreate(RejectCreateArguments {
        project_identifier: Some(verb),
    })
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
        mode: pwf_application::pending_work::ListMode::ProjectRoute,
    }
}

pub(super) fn run_reject_create(
    arguments: &RejectCreateArguments,
    projects: &ProjectRegistry,
) -> Result<String, PendingWorkError> {
    reject_pending_work_create::execute(
        &RejectPendingWorkCreate {
            project_identifier: arguments.project_identifier.clone(),
        },
        projects,
    )?;
    Err(PendingWorkError::RouteCreateRejected)
}

#[cfg(test)]
mod tests {
    use pwf_application::pending_work::{ListMode, StatusFilter};

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
    fn verify_route_builds_the_compatibility_arguments() {
        let ResolvedCommand::Verify(verify) = resolve(&arguments(&["verify", "cfg57"])) else {
            panic!("expected routed verify");
        };

        assert_eq!(verify.identifier.raw(), Some("cfg57"));
        assert_eq!(verify.agent, AgentChoice::Claude);
        assert_eq!(verify.model, None);
    }

    #[test]
    fn create_and_multiword_routes_reach_the_rejection_owner() {
        let ResolvedCommand::RejectCreate(add) = resolve(&arguments(&["add"])) else {
            panic!("expected rejected create");
        };
        assert_eq!(add.project_identifier, None);

        let ResolvedCommand::RejectCreate(project_prompt) =
            resolve(&arguments(&["pwf", "build", "it"]))
        else {
            panic!("expected rejected create");
        };
        assert_eq!(project_prompt.project_identifier.as_deref(), Some("pwf"));
    }
}
