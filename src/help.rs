//! Custom help surfaces clap's derive does not provide: the token-lean `--terse`
//! agent help plus the small token predicates `main` uses before handing rich
//! help to clap. Rich help is rendered from `command.rs`.

// Terse help: verbs + required args only, no prose/recipe-hints/route-shortcuts.
// Tuned for AI agents driving the engine (the skills point them here, not at the
// rich `--help`), so keep it token-lean.
const PW_TERSE: &str = r"<project> [-n <N>] [--long|--future|--human|--all]   (routes to that project's open items; `pwf list` lists every project)
  list [-n <N>] [--long] [--future] [--human] [--all] [-o/--order <created|id|project-id> <asc|desc>]   (--order default: created desc, flat across every project; --order project-id reproduces the pre-PWF-0096 project-grouped default)
  add <project> <prompt>   prompt lanes: <title> / <goal> /c <context> /n <constraint> /d <done>; plus [--title] [--human] [--section <s>] [--prereq <id>] [--continue-handoff] [--continue <path>]
  done --id [--report] [--commits <range>] [--review]
  cancel --id --report [--commits <range>] [--review]
  reopen --id   (inverse of done/cancel: done|cancelled -> active)
  update --id [--prompt] [--title] [--prereq <id>] [--clear-prereq] [--commits <range>] [--append-report <md>] [-a/--append <lanes>]   (--commits/--append-report also amend a closed item; -a/--append splices lane-syntax bullets into Goals/Context/Constraints/Done When, conflicts with --prompt)
  resolve --id [--show]
  show <id>   (shorthand for resolve --show)
  session <id> [--agent claude|codex] [--model <name>] [-a/--append <lanes>] [-i] [-w] [--auto] [-y]   dispatch an agent into the project's zellij session, or inline in the current terminal with -i (--agent picks the agent, claude default; --model forwards a raw model override to the agent's own --model flag, no validation, wins over effort-tier resolution; -a/--append splices lane-syntax bullets into the body before dispatch, same as update; -w tells it to work in a git worktree named after the id; --auto runs it autonomously without prompting the user; -y skips the [Y/n] confirm)
  clean [--dry-run|--force]
  verify [--id] [-a claude|codex] [--model <name>]
  remove --id";

const HANDOFF_TERSE: &str = r"handoff <verb> [--repo-root <path>]
  refresh
  new [--title] [--slug]
  done --id
  cancel --id
  reopen --id   (inverse of done/cancel: un-archives + reopens the linked pw item)
  list";

const MIGRATE_TERSE: &str =
    r"migrate [--config-path <path>]   (migrates flat <project>.md into <project>/<project>.md)";

const NOTE_TERSE: &str = r"note <project> [verb]
  ls [-n <N>]
  add <message>
  update <id> <message>
  remove <id>";

/// Terse, token-lean help for one non-default engine (`handoff`, `migrate`,
/// `note`). `None` for anything else — pending-work verbs aren't a named
/// engine to scope to; they resolve individually via `terse_verb`, or in full
/// as part of `terse_text`.
pub fn terse_engine(engine: &str) -> Option<String> {
    let block = match engine.to_ascii_lowercase().as_str() {
        "handoff" => HANDOFF_TERSE,
        "migrate" => MIGRATE_TERSE,
        "note" => NOTE_TERSE,
        _ => return None,
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
        .skip(1) // first line is the `<project>` route-shorthand header, not a verb
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
    fn terse_engine_has_no_pending_work_grouping() {
        // Pending-work verbs are flattened, top-level clap subcommands — there's no
        // named "pw" engine left to scope terse help to (`terse_verb` covers one
        // verb at a time; `terse_text` covers all of them as part of everything).
        assert!(terse_engine("pw").is_none());
        assert!(terse_engine("pending-work").is_none());
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
        // ...and keeps verbs + required args for the rest, matching the old
        // whole-block assertions now that there's no "pw" grouping to fetch them from.
        assert!(
            terse_verb("add")
                .unwrap()
                .contains("add <project> <prompt>")
        );
        assert!(terse_verb("done").unwrap().contains("done --id [--report]"));
        assert!(
            terse_verb("cancel")
                .unwrap()
                .contains("cancel --id --report")
        );
        assert!(terse_verb("resolve").unwrap().contains("resolve --id"));
        // PWF-0065: the `show` shorthand for `resolve --show` is its own terse line,
        // taking a bare positional id.
        assert!(terse_verb("show").unwrap().contains("show <id>"));
        assert!(terse_verb("remove").unwrap().contains("remove --id"));
        // The route-shorthand header is not a verb.
        assert_eq!(terse_verb("pw"), None);
        assert_eq!(terse_verb("bogus"), None);
    }

    #[test]
    fn terse_text_covers_all_engines() {
        let t = terse_text();
        // ...keeps verbs + required args...
        assert!(t.contains("add <project> <prompt>"));
        assert!(t.contains("done --id [--report]"));
        // ...but drops human-only prose: descriptions, recipe hints, route shortcuts.
        assert!(!t.contains("[just "), "terse must drop recipe hints");
        assert!(
            !t.contains("List open items."),
            "terse must drop descriptions"
        );
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
