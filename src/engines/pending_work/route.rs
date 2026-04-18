// `pw` word-based routing: the read-only sub-dispatcher behind `pwf pw route` (and
// bare `pwf pw <words…>`). Create paths were removed in PWF-0034 — the only
// create form is `pwf pw add <project> "<prompt>"`; bare words error with a hint.

use super::actions::run_list_action;
use super::claude::{invoke_claude_launch, verify_json_with_probe, RealProbe};
use super::errors::PendingWorkError;
use super::query::{find_pending_item, resolve_managed_project_name_typed};
use crate::cli::Args;
use crate::config::Config;

pub(super) fn run_route(cfg: &Config, args: &Args, date: &str) -> Result<String, PendingWorkError> {
    let route_words: Vec<&str> = args
        .words
        .iter()
        .map(|s| s.as_str())
        .filter(|s| !s.trim().is_empty())
        .collect();

    if route_words.is_empty() {
        // list all
        return run_list_action(
            cfg,
            None,
            args.json,
            args.long,
            args.future,
            args.human,
            args.number,
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
        let probe = RealProbe::resolve();
        let item = if let Some(vid) = verify_id {
            Some(find_pending_item(cfg, vid)?)
        } else {
            None
        };
        return Ok(verify_json_with_probe(item.as_ref(), &probe));
    }

    if verb == "launch-claude" || verb == "lc" {
        if route_words.len() < 2 {
            return Err(PendingWorkError::RouteLaunchClaudeUsage);
        }
        let probe = RealProbe::resolve();
        return invoke_claude_launch(cfg, route_words[1], &probe, args.force);
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
            args.json,
            args.force,
            &crate::engines::clean::RealConfirm,
        )?);
    }

    // Otherwise: treat word[0] as a project name. A single word lists that
    // project; trailing words error (create is `pw add` only — no silent route create).
    let project_name = resolve_managed_project_name_typed(cfg, &verb)?;
    if route_words.len() == 1 {
        return run_list_action(
            cfg,
            Some(&project_name),
            args.json,
            args.long,
            args.future,
            args.human,
            args.number,
        );
    }

    Err(PendingWorkError::RouteCreateRejected)
}

#[cfg(test)]
mod tests {
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

        assert!(matches!(err, errors::PendingWorkError::RouteCreateRejected));
        assert_eq!(err.to_string(), errors::ADD_HINT);
    }

    #[test]
    fn route_launch_claude_without_id_returns_typed_usage_error() {
        let cfg = cfg();

        let err = run_route(&cfg, &args(&["launch-claude"]), "2026-01-01").unwrap_err();

        assert!(matches!(
            err,
            errors::PendingWorkError::RouteLaunchClaudeUsage
        ));
        assert_eq!(err.to_string(), "Usage: pwf pw launch-claude --id <id>");
    }
}
