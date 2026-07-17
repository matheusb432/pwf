//! Routes legacy word forms without supporting task creation.

use pwf_application::{
    AppDbStore, PendingWorkItem,
    pending_work::session::{Agent, ModelTierCatalog, SessionRuntime, verify::VerifySession},
};
use pwf_domain::pending_work::ProjectRegistry;

use super::{
    actions::{
        ListParams,
        list::{ListScope, OrderDirection, OrderField, OrderSpec},
        run_list_query,
    },
    agent::verify,
    color::use_color,
    errors::PendingWorkError,
    naming::stamp_date,
    query::resolve_managed_project_name_typed,
};
use crate::{
    cli::EngineArgs,
    config::Config,
    confirm::{Confirmation, DefaultAnswer},
};

/// Preserves project grouping for legacy word-routed lists.
const ROUTE_ORDER: OrderSpec = OrderSpec {
    field: OrderField::ProjectId,
    direction: OrderDirection::Asc,
};

pub(super) fn run_route(
    cfg: &Config,
    store: &impl AppDbStore<PendingWorkItem>,
    projects: &ProjectRegistry,
    model_tiers: &impl ModelTierCatalog,
    runtime: &impl SessionRuntime,
    args: &EngineArgs,
    confirmation: &impl Fn(&str, DefaultAnswer) -> Confirmation,
) -> Result<String, PendingWorkError> {
    let route_words: Vec<&str> = args
        .words
        .iter()
        .map(std::string::String::as_str)
        .filter(|s| !s.trim().is_empty())
        .collect();

    if route_words.is_empty() {
        let scope = ListScope::from_flags(args.human, args.future, args.all)?;
        return run_list_query(
            cfg,
            store,
            ListParams {
                only_project: None,
                long: args.long,
                scope,
                number: args.number,
                effort: args.effort,
                tags: None,
                order: ROUTE_ORDER,
                status_filter: args.status_filter,
                color_on: use_color(args.color),
            },
        );
    }

    let verb = route_words[0].to_ascii_lowercase();

    // Bare create forms remain rejected because they can create duplicates.
    if matches!(verb.as_str(), "add" | "a" | "add-titled" | "at") {
        return Err(PendingWorkError::RouteCreateRejected);
    }

    if verb == "verify" || verb == "v" {
        let verify_id = if route_words.len() >= 2 {
            Some(route_words[1])
        } else {
            None
        };
        let request = VerifySession {
            id: verify_id.map(str::to_string),
            agent: Agent::Claude,
            model_override: None,
        };
        let outcome = pwf_application::pending_work::session::verify::execute(
            request,
            store,
            projects,
            model_tiers,
            runtime,
        )?;
        return Ok(verify::render(&outcome));
    }

    if verb == "clean" || verb == "cl" {
        let only = if route_words.len() >= 2 {
            Some(resolve_managed_project_name_typed(cfg, route_words[1])?)
        } else {
            None
        };
        return Ok(crate::engines::clean::run_clean_typed(
            cfg,
            only.as_deref(),
            &stamp_date(args.date.as_deref()),
            args.dry_run,
            args.force,
            confirmation,
        )?);
    }

    // A single project word lists; trailing words are rejected as removed create syntax.
    let project_name = resolve_managed_project_name_typed(cfg, &verb)?;
    if route_words.len() == 1 {
        let scope = ListScope::from_flags(args.human, args.future, args.all)?;
        return run_list_query(
            cfg,
            store,
            ListParams {
                only_project: Some(&project_name),
                long: args.long,
                scope,
                number: args.number,
                effort: args.effort,
                tags: None,
                order: ROUTE_ORDER,
                status_filter: args.status_filter,
                color_on: use_color(args.color),
            },
        );
    }

    Err(PendingWorkError::RouteCreateRejected)
}

#[cfg(test)]
mod tests {
    use std::{assert_matches, convert::Infallible};

    use pwf_application::pending_work::session::{
        AgentLaunch, AgentProbe, DispatchTarget, ModelTierLookup, TabOpenError,
    };
    use pwf_domain::pending_work::EffortTier;

    use super::*;
    use crate::engines::pending_work::errors;

    #[derive(Clone, Copy)]
    struct UnusedModelTiers;

    impl ModelTierCatalog for UnusedModelTiers {
        type Error = Infallible;

        fn tier(&self, _effort: EffortTier) -> Result<ModelTierLookup, Self::Error> {
            unreachable!("create-route rejection does not read model tiers")
        }
    }

    #[derive(Clone, Copy)]
    struct UnusedRuntime;

    impl SessionRuntime for UnusedRuntime {
        fn probe_agent(&self, _agent: Agent) -> AgentProbe {
            unreachable!("create-route rejection does not probe an agent")
        }

        fn repository_is_directory(&self, _path: &str) -> bool {
            unreachable!("create-route rejection does not inspect a repository")
        }

        fn multiplexer_available(&self) -> bool {
            unreachable!("create-route rejection does not inspect the multiplexer")
        }

        fn command_preview(&self, _launch: &AgentLaunch) -> String {
            unreachable!("create-route rejection does not render a command")
        }

        fn run_inline(&self, _launch: &AgentLaunch) -> Result<(), String> {
            unreachable!("create-route rejection does not dispatch")
        }

        fn open_tab(
            &self,
            _target: &DispatchTarget,
            _launch: &AgentLaunch,
        ) -> Result<(), TabOpenError> {
            unreachable!("create-route rejection does not dispatch")
        }

        fn ensure_session(&self, _session: &str) -> Result<(), String> {
            unreachable!("create-route rejection does not dispatch")
        }
    }

    fn cfg() -> Config {
        crate::config::from_json(
            r#"{ "notesDir": "/tmp/pwf-route-notes", "projects": { "glep-shimeji": "/repo" }, "prefixes": { "glep-shimeji": "GLP" } }"#,
            None,
        )
        .unwrap()
    }

    fn args(words: &[&str]) -> EngineArgs {
        EngineArgs {
            words: words.iter().map(|word| (*word).to_string()).collect(),
            ..EngineArgs::default()
        }
    }

    #[test]
    fn removed_create_route_returns_typed_error_with_add_hint() {
        let cfg = cfg();
        let store = super::super::store_for(&cfg);

        let err = run_route(
            &cfg,
            &store,
            &super::super::run::project_registry(&cfg),
            &UnusedModelTiers,
            &UnusedRuntime,
            &args(&["add", "glep-shimeji", "do it"]),
            &|_, _| Confirmation::NonInteractive,
        )
        .unwrap_err();

        assert_matches!(err, errors::PendingWorkError::RouteCreateRejected);
        assert_eq!(err.to_string(), errors::ADD_HINT);
    }
}
