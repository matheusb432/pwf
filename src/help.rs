//! Custom help surfaces clap's derive does not provide: the token-lean `--terse`
//! agent help plus the small token predicates `main` uses before handing rich
//! help to clap. Rich help is rendered from `command.rs`.

// Terse help: verbs + required args only, no prose/recipe-hints/route-shortcuts.
// Tuned for AI agents driving the engine (the skills point them here, not at the
// rich `--help`), so keep it token-lean.
const PW_TERSE: &str = r#"pw [<project>]   (alias: pending-work; bare pw lists all; pw <project> [-n <N>] [--long|--future|--human|--all] lists that project)
  list [-n <N>] [--long] [--future] [--human] [--all]
  add <project> <prompt>   prompt lanes: <title> / <goal> /c <context> /n <constraint> /d <done>; plus [--title] [--human] [--section <s>] [--prereq <id>] [--continue-handoff] [--continue <path>]
  check --id [--report] [--commits <range>] [--review]
  cancel --id --report [--commits <range>] [--review]
  reopen --id   (inverse of check/cancel: done|cancelled -> active)
  update --id [--prompt] [--title] [--prereq <id>] [--clear-prereq] [--commits <range>] [--append-report <md>]   (--commits/--append-report also amend a closed item)
  resolve --id [--show]
  show <id>   (shorthand for resolve --show)
  session <id> [-a claude|codex] [-i] [-w] [--auto] [-y]   dispatch an agent into the project's zellij session, or inline in the current terminal with -i (-a picks the agent, claude default; -w tells it to work in a git worktree named after the id; --auto runs it autonomously without prompting the user; -y skips the [Y/n] confirm)
  clean [--dry-run|--force]
  verify [--id] [-a claude|codex]
  remove --id"#;

const HANDOFF_TERSE: &str = r#"handoff <verb> [--repo-root <path>]
  refresh
  new [--title] [--slug]
  done --id
  cancel --id
  reopen --id   (inverse of done/cancel: un-archives + reopens the linked pw item)
  list"#;

const MIGRATE_TERSE: &str =
    r#"migrate [--config-path <path>]   (migrates flat <project>.md into <project>/<project>.md)"#;

const NOTE_TERSE: &str = r#"note <project> [verb]
  ls [-n <N>]
  add <message>
  update <id> <message>
  remove <id>"#;

/// Terse, token-lean help for one engine (verbs + required args). `None` for an
/// unknown engine; the `pw`/`pending-work` alias resolves via `Engine::from_str`.
pub fn terse_engine(engine: &str) -> Option<String> {
    use crate::engines::Engine;
    let block = match engine.to_ascii_lowercase().as_str() {
        "note" => NOTE_TERSE,
        other => match other.parse::<Engine>().ok()? {
            Engine::PendingWork => PW_TERSE,
            Engine::Handoff => HANDOFF_TERSE,
            Engine::Migrate => MIGRATE_TERSE,
        },
    };
    Some(block.to_string())
}

/// Terse help for every engine (top-level `pwf --help --terse`).
pub fn terse_text() -> String {
    format!("{PW_TERSE}\n\n{HANDOFF_TERSE}\n\n{MIGRATE_TERSE}\n\n{NOTE_TERSE}")
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

/// True if `tok` is a rich-help token or the legacy `--list` top-level alias.
pub fn is_help_token(tok: &str) -> bool {
    matches!(
        tok.to_ascii_lowercase().as_str(),
        "--help" | "-h" | "help" | "--list"
    )
}

/// True for the `--terse` help-format modifier.
pub fn is_terse(tok: &str) -> bool {
    tok.eq_ignore_ascii_case("--terse")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_token_matches_rich_help_tokens_and_list_alias() {
        for t in ["--help", "-h", "help", "--list", "--HELP", "-H"] {
            assert!(is_help_token(t), "'{t}' should be a help token");
        }
        for t in ["--long", "add", "list", "pw"] {
            assert!(!is_help_token(t), "'{t}' should not be a help token");
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
        // PWF-0065: the `show` shorthand for `resolve --show` is its own terse line,
        // taking a bare positional id.
        assert!(terse.contains("show <id>"));
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
        assert!(terse_engine("note").unwrap().contains("add <message>"));
        assert!(
            terse_engine("note")
                .unwrap()
                .contains("update <id> <message>")
        );
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
        assert!(t.contains("note <project>"));
    }

    #[test]
    fn terse_and_help_request_tokens() {
        for t in ["--terse", "--TERSE"] {
            assert!(is_terse(t), "'{t}' should be the terse modifier");
        }
        assert!(!is_terse("--help"));
        assert!(is_help_token("--help"));
        assert!(!is_help_token("list"));
    }
}
