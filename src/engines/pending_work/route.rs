// `pw` word-based routing: the read-only sub-dispatcher behind `pwf route` (and
// bare `pwf <words…>`). Create paths were removed in PWF-0034 — the only
// create form is `pwf add <project> "<prompt>"`; bare words error with a hint.

use super::{
    actions::{
        list::{ListScope, OrderDirection, OrderField, OrderSpec},
        run_list_action,
    },
    agent::{probe::RealProbe, verify::verify_text_with_probe},
    color::use_color,
    errors::PendingWorkError,
    query::{find_pending_item, resolve_managed_project_name_typed},
};
use crate::{cli::Args, config::Config};

/// Fixed legacy order for the `route` word-router (bare `pwf`/`pwf <project>`):
/// project-name ascending, then id-suffix descending within a project — never
/// affected by `--order`, since `route` has no `--order` flag to forward.
const ROUTE_ORDER: OrderSpec = OrderSpec {
    field: OrderField::ProjectId,
    direction: OrderDirection::Asc,
};

pub(super) fn run_route(cfg: &Config, args: &Args, date: &str) -> Result<String, PendingWorkError> {
    let route_words: Vec<&str> = args
        .words
        .iter()
        .map(|s| s.as_str())
        .filter(|s| !s.trim().is_empty())
        .collect();

    if route_words.is_empty() {
        // list all
        let scope = ListScope::from_flags(args.human, args.future, args.all)?;
        return run_list_action(
            cfg,
            None,
            args.long,
            scope,
            args.number,
            args.effort,
            ROUTE_ORDER,
            use_color(args.color),
        );
    }

    let verb = route_words[0].to_ascii_lowercase();

    // ! Create via bare words / sub-verbs was the duplicate-item footgun (PWF-0034).
    if matches!(verb.as_str(), "add" | "a" | "add-titled" | "at") {
        return Err(PendingWorkError::RouteCreateRejected);
    }

    if verb == "verify" || verb == "v" {
        let verify_id = if route_words.len() >= 2 {
            Some(route_words[1])
        } else {
            None
        };
        let launcher = super::session::launcher_for(crate::cli::Agent::Claude);
        let probe = RealProbe::resolve(launcher.binary());
        let item = if let Some(vid) = verify_id {
            Some(find_pending_item(cfg, vid)?)
        } else {
            None
        };
        let claude_model = item.as_ref().and_then(|it| {
            super::session::resolve_model_for_verify(crate::cli::Agent::Claude, it, None)
        });
        return Ok(verify_text_with_probe(
            item.as_ref(),
            launcher,
            &probe,
            claude_model,
        ));
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
            date,
            args.dry_run,
            args.force,
            &crate::confirm::RealConfirm,
        )?);
    }

    // Otherwise: treat word[0] as a project name. A single word lists that
    // project; trailing words error (create is `pw add` only — no silent route create).
    let project_name = resolve_managed_project_name_typed(cfg, &verb)?;
    if route_words.len() == 1 {
        let scope = ListScope::from_flags(args.human, args.future, args.all)?;
        return run_list_action(
            cfg,
            Some(&project_name),
            args.long,
            scope,
            args.number,
            args.effort,
            ROUTE_ORDER,
            use_color(args.color),
        );
    }

    Err(PendingWorkError::RouteCreateRejected)
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use super::*;
    use crate::engines::pending_work::errors;

    fn cfg() -> Config {
        crate::config::from_json(
            r#"{ "notesDir": "/tmp/pwf-route-notes", "projects": { "glep-shimeji": "/repo" }, "prefixes": { "glep-shimeji": "GLP" } }"#,
            None,
        )
        .unwrap()
    }

    fn args(words: &[&str]) -> Args {
        Args {
            words: words.iter().map(|word| (*word).to_string()).collect(),
            ..Args::default()
        }
    }

    #[test]
    fn removed_create_route_returns_typed_error_with_add_hint() {
        let cfg = cfg();

        let err =
            run_route(&cfg, &args(&["add", "glep-shimeji", "do it"]), "2026-01-01").unwrap_err();

        assert_matches!(err, errors::PendingWorkError::RouteCreateRejected);
        assert_eq!(err.to_string(), errors::ADD_HINT);
    }
}
