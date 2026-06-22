//! Custom help surfaces clap's derive does not provide: the token-lean `--terse`
//! agent help, plus the `--help`/`-h`/`help`/`--list` and `--terse` token
//! predicates `main` uses to route to clap-rendered help vs the terse text. The
//! rich per-command help is now derived by clap (`command.rs`) — the single
//! source of truth (PWF-0030).

// Terse help: verbs + required args only, no prose/recipe-hints/route-shortcuts.
// Tuned for AI agents driving the engine (the skills point them here, not at the
// rich `--help`), so keep it token-lean.
const PW_TERSE: &str = r#"pw [<project>]   (alias: pending-work; bare pw lists all; pw <project> [-n <N>] [--long|--future|--human|--all] lists that project)
  list [-n <N>] [--long] [--future] [--human] [--all]
  add <project> <prompt> [--title] [--human] [--section <s>] [--prereq <id>] [--continue-handoff] [--continue <path>]
  check --id [--report] [--commits <range>] [--review]
  cancel --id --report [--commits <range>] [--review]
  update --id [--prompt] [--title] [--prereq <id>] [--clear-prereq] [--commits <range>]
  resolve --id [--show]
  show --id   (shorthand for resolve --show)
  clean [--dry-run|--force]
  verify [--id]
  launch --id
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

/// Curated top-level rich help. Per-command and per-engine detail still comes
/// from clap (`pwf <verb> --help`, `pwf handoff --help`); this page is the
/// readable map for the default pending-work surface plus the non-default engines.
pub fn rich_top_help() -> &'static str {
    r#"pwf - pending-work / handoff / migrate

USAGE
  pwf <pending-work command> [args]
  pwf <project> [-n <N>] [--long|--future|--human|--all]
  pwf <engine> <command> [args]

PENDING-WORK COMMANDS (default engine)
  add <project> <prompt>       Create a task
  list                         List open tasks
  <project>                    List one project's open tasks
  check --id <id>              Mark a task done
  cancel --id <id> --report    Mark a task cancelled
  update --id <id>             Replace task text/title/prereqs or amend commits
  resolve --id <id>            Print the task note path
  show --id <id>               Stream the task note (alias for resolve --show)
  clean                        Archive or clear done tasks
  verify --id <id>             Probe whether a task can launch
  launch --id <id>             Emit a launch spec
  launch-claude --id <id>      Emit a direct claude launch
  remove --id <id>             Delete a task note and index link

ENGINES
  handoff                      Per-repo handoff ledgers
  migrate                      Migrate a flat project note into the folder model

HELP
  pwf <command> --help         Detailed pending-work command help
  pwf handoff --help           Handoff command help
  pwf migrate --help           Migrate command help
  pwf --help --terse           Token-lean agent help
"#
}

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

/// Terse help for one pending-work verb (e.g. `update`): the single matching line
/// from `PW_TERSE`, trimmed. `None` if `verb` is not a pending-work verb. Lets
/// `pwf <verb> --help --terse` scope to that verb instead of dumping every engine.
pub fn terse_verb(verb: &str) -> Option<String> {
    PW_TERSE
        .lines()
        .skip(1) // first line is the `pw [<project>]` engine header, not a verb
        .map(str::trim)
        .find(|line| {
            line.split([' ', '\t'])
                .next()
                .is_some_and(|head| head.eq_ignore_ascii_case(verb))
        })
        .map(str::to_string)
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
        assert!(terse.contains("cancel --id --report"));
        assert!(!terse.contains("launch-orca"), "orca verbs are removed");
        assert!(terse.contains("resolve --id"));
        // PWF-0065: the `show` shorthand for `resolve --show` is its own terse line.
        assert!(terse.contains("show --id"));
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
    fn terse_verb_scopes_to_one_pending_work_verb() {
        // A verb returns only its own line — no other verbs, no other engines.
        let u = terse_verb("update").expect("update verb");
        assert!(u.starts_with("update --id"));
        assert!(!u.contains("resolve"), "must not bleed other verbs: {u}");
        assert!(!u.contains("handoff"), "must not bleed engines: {u}");
        // Case-insensitive.
        assert_eq!(terse_verb("UPDATE"), terse_verb("update"));
        // `launch` matches its own line, not the `launch-claude` line.
        assert_eq!(terse_verb("launch").as_deref(), Some("launch --id"));
        assert!(terse_verb("launch-claude").unwrap().contains("--force"));
        // The `pw [<project>]` header is not a verb.
        assert_eq!(terse_verb("pw"), None);
        assert_eq!(terse_verb("bogus"), None);
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
