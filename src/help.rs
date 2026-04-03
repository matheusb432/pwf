//! Custom help surfaces clap's derive does not provide: the token-lean `--terse`
//! agent help, plus the `--help`/`-h`/`help`/`--list` and `--terse` token
//! predicates `main` uses to route to clap-rendered help vs the terse text. The
//! rich per-command help is now derived by clap (`command.rs`) — the single
//! source of truth (PWF-0030).

// Terse help: verbs + required args only, no prose/recipe-hints/route-shortcuts.
// Tuned for AI agents driving the engine (the skills point them here, not at the
// rich `--help`), so keep it token-lean.
const PW_TERSE: &str = r#"pw [<project>]   (alias: pending-work; bare pw lists all; pw <project> [-n <N>] [--long|--future|--human] lists that project)
  list [-n <N>] [--long] [--future] [--human]
  add <project> <prompt> [--title] [--human] [--section <s>] [--prereq <id>] [--continue-handoff] [--continue <path>]
  check --id [--report] [--commits <range>] [--review]
  update --id [--prompt] [--title] [--prereq <id>] [--clear-prereq]
  resolve --id [--json]
  clean [--dry-run|--force]
  verify [--id]
  launch --id [--json]
  launch-claude --id [--force]
  remove --id"#;

const HANDOFF_TERSE: &str = r#"handoff <verb> [--repo-root <path>]
  refresh
  new [--title] [--slug]
  done --id
  cancel --id
  list"#;

const MIGRATE_TERSE: &str =
    r#"migrate [--config-path <path>]   (migrates flat <project>.md into <project>/<project>.md)"#;

/// Terse, token-lean help for one engine (verbs + required args). `None` for an
/// unknown engine; the `pw`/`pending-work` alias resolves via `Engine::from_str`.
pub fn terse_engine(engine: &str) -> Option<String> {
    use crate::engines::Engine;
    let block = match engine.parse::<Engine>().ok()? {
        Engine::PendingWork => PW_TERSE,
        Engine::Handoff => HANDOFF_TERSE,
        Engine::Migrate => MIGRATE_TERSE,
    };
    Some(block.to_string())
}

/// Terse help for every engine (top-level `pwf --help --terse`).
pub fn terse_text() -> String {
    format!("{PW_TERSE}\n\n{HANDOFF_TERSE}\n\n{MIGRATE_TERSE}")
}

/// True if `tok` requests help (`--help`/`-h`/`help`) or its `--list` alias.
fn wants_help(tok: &str) -> bool {
    matches!(
        tok.to_ascii_lowercase().as_str(),
        "--help" | "-h" | "help" | "--list"
    )
}

/// True for the `--terse` help-format modifier.
pub fn is_terse(tok: &str) -> bool {
    tok.eq_ignore_ascii_case("--terse")
}

/// True if `tok` requests help in any form — `wants_help` tokens or `--terse`.
pub fn help_request(tok: &str) -> bool {
    wants_help(tok) || is_terse(tok)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wants_help_matches_help_tokens_and_list_alias() {
        for t in ["--help", "-h", "help", "--list", "--HELP", "-H"] {
            assert!(wants_help(t), "'{t}' should be a help token");
        }
        for t in ["--long", "add", "list", "pw"] {
            assert!(!wants_help(t), "'{t}' should not be a help token");
        }
    }

    #[test]
    fn terse_engine_is_lean_and_scoped() {
        let terse = terse_engine("pw").expect("pw terse");
        // Keeps verbs + required args...
        assert!(terse.contains("add <project> <prompt>"));
        assert!(terse.contains("check --id [--report]"));
        assert!(!terse.contains("launch-orca"), "orca verbs are removed");
        assert!(terse.contains("resolve --id"));
        // ...but drops human-only prose: descriptions, recipe hints, route shortcuts.
        assert!(terse.contains("remove --id"));
        assert!(!terse.contains("[just "), "terse must drop recipe hints");
        assert!(
            !terse.contains("List open items."),
            "terse must drop descriptions"
        );
        assert!(
            !terse.contains("Route shortcuts"),
            "terse must drop route shortcuts"
        );
    }

    #[test]
    fn terse_engine_scoped_alias_and_unknown() {
        assert!(terse_engine("handoff").unwrap().contains("done --id"));
        assert!(!terse_engine("handoff").unwrap().contains("launch-claude"));
        assert_eq!(terse_engine("pw"), terse_engine("pending-work"));
        assert!(terse_engine("bogus").is_none());
    }

    #[test]
    fn terse_text_covers_all_engines() {
        let t = terse_text();
        assert!(t.contains("pw [<project>"));
        assert!(t.contains("handoff <verb>"));
        assert!(t.contains("migrate"));
    }

    #[test]
    fn terse_and_help_request_tokens() {
        for t in ["--terse", "--TERSE"] {
            assert!(is_terse(t), "'{t}' should be the terse modifier");
            assert!(help_request(t), "'{t}' should be a help request");
        }
        assert!(!is_terse("--help"));
        assert!(
            help_request("--help"),
            "wants_help still triggers a request"
        );
        assert!(!help_request("list"));
    }
}
