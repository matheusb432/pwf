//! Provides token-lean `--terse` help and pre-clap help-token detection.
//! Rich help remains derived from `command.rs`.

// Keep terse help to verbs, required arguments, and routing-critical qualifiers.
const PW_TERSE: &str = r"<project> [-n <N>] [--status <active|done|cancelled|all>] [--long|--future|--human|--all]   (routes to that project's pending-work items; `pwf list` lists every project)
  list [-n <N>] [--status <active|done|cancelled|all>] [--long] [--future] [--human] [--all] [--tag <tag>] [-o/--order <created|id|project-id> <asc|desc>]   (--all includes every section; --status all includes every lifecycle; --order default: created desc, flat across every project; --order project-id reproduces the pre-PWF-0096 project-grouped default)
  add <project> <prompt>   prompt lanes: <title> / <goal> /c <context> /n <constraint> /d <done>; plus [--title] [--human] [--section <s>] [--prereq <id>] [--tag <tag>] [--continue-handoff] [--continue <path>]
  done --id [--report] [--commits <range>] [--review]   (handoff-tagged items auto-archive their handoff)
  cancel --id --report [--commits <range>] [--review]   (handoff-tagged items auto-archive their handoff)
  reopen --id   (inverse of done/cancel: done|cancelled -> active; handoff-tagged items auto-restore their handoff)
  update --id [--prompt] [--title] [--prereq <id>] [--clear-prereq] [--tag <tag>] [--tags-clear] [--commits <range>] [--append-report <md>] [-a/--append <lanes>]   (--commits/--append-report also amend a closed item; -a/--append splices lane-syntax bullets into Goals/Context/Constraints/Done When, conflicts with --prompt)
  resolve --id [--show]
  show <id>   (shorthand for resolve --show)
  session <id> [--agent claude|codex] [--model <name>] [-a/--append <lanes>] [-i] [-w] [--auto] [-y]   dispatch an agent into the project's zellij session, or inline in the current terminal with -i (--agent picks the agent, claude default; --model forwards a raw model override to the agent's own --model flag, no validation, wins over effort-tier resolution; -a/--append splices lane-syntax bullets into the body before dispatch, same as update; -w tells it to work in a git worktree named after the id; --auto runs it autonomously without prompting the user; -y skips the [Y/n] confirm)
  clean [--dry-run|--force]
  verify [--id] [-a claude|codex] [--model <name>]
  remove --id";

const HANDOFF_TERSE: &str = r"handoff <verb> [--repo-root <path>]
  add [--title] [--slug]
  list";

const MIGRATE_TERSE: &str =
    r"migrate [--config-path <path>]   (migrates flat <project>.md into <project>/<project>.md)";

const NOTE_TERSE: &str = r"note <project> [verb]
  ls [-n <N>]
  add <message>
  update <id> <message>
  remove <id>";

const RENAME_PROJECT_TERSE: &str = r"rename-project --old <CODE> --new <CODE> [--new-path <path>] [--dry-run]   (relocate a project's pwf-db identity + repos.toml entry; --dry-run previews the full plan)";

/// Returns terse help for `handoff`, `migrate`, or `note`.
/// Pending-work verbs are resolved by [`terse_verb`] instead.
pub fn terse_engine(engine: &str) -> Option<String> {
    let block = match engine.to_ascii_lowercase().as_str() {
        "handoff" => HANDOFF_TERSE,
        "migrate" => MIGRATE_TERSE,
        "note" => NOTE_TERSE,
        "rename-project" => RENAME_PROJECT_TERSE,
        _ => return None,
    };
    Some(block.to_string())
}

/// Returns terse help for every engine.
pub fn terse_text() -> String {
    format!(
        "{PW_TERSE}\n\n{HANDOFF_TERSE}\n\n{MIGRATE_TERSE}\n\n{NOTE_TERSE}\n\n{RENAME_PROJECT_TERSE}"
    )
}

/// Returns the trimmed terse-help line for one pending-work verb.
pub fn terse_verb(verb: &str) -> Option<String> {
    PW_TERSE
        .lines()
        .skip(1) // Skip the route header.
        .map(str::trim)
        .find(|line| {
            line.split([' ', '\t'])
                .next()
                .is_some_and(|head| head.eq_ignore_ascii_case(verb))
        })
        .map(str::to_string)
}

/// Reports whether `tok` requests rich help or the legacy top-level list alias.
pub fn is_help_token(tok: &str) -> bool {
    matches!(
        tok.to_ascii_lowercase().as_str(),
        "--help" | "-h" | "help" | "--list"
    )
}

/// Reports whether `tok` requests terse help.
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
        assert!(terse_engine("pw").is_none());
        assert!(terse_engine("pending-work").is_none());
    }

    #[test]
    fn terse_engine_scoped_alias_and_unknown() {
        assert!(terse_engine("handoff").unwrap().contains("add [--title]"));
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
        let u = terse_verb("update").expect("update verb");
        assert!(u.starts_with("update --id"));
        assert!(!u.contains("resolve"), "must not bleed other verbs: {u}");
        assert!(!u.contains("handoff"), "must not bleed engines: {u}");
        assert_eq!(terse_verb("UPDATE"), terse_verb("update"));
        assert!(
            terse_verb("add")
                .unwrap()
                .contains("add <project> <prompt>")
        );
        assert!(terse_verb("done").unwrap().contains("done --id [--report]"));
        assert!(
            terse_verb("done")
                .unwrap()
                .contains("handoff-tagged items auto-archive their handoff")
        );
        assert!(
            terse_verb("cancel")
                .unwrap()
                .contains("cancel --id --report")
        );
        assert!(
            terse_verb("cancel")
                .unwrap()
                .contains("handoff-tagged items auto-archive their handoff"),
            "cancel should carry the same handoff-mirroring fact as done"
        );
        assert!(
            terse_verb("reopen")
                .unwrap()
                .contains("handoff-tagged items auto-restore their handoff"),
            "reopen should note it mirrors onto the handoff too"
        );
        assert!(terse_verb("resolve").unwrap().contains("resolve --id"));
        assert!(terse_verb("show").unwrap().contains("show <id>"));
        assert!(terse_verb("remove").unwrap().contains("remove --id"));
        assert_eq!(terse_verb("pw"), None);
        assert_eq!(terse_verb("bogus"), None);
    }

    #[test]
    fn terse_text_covers_all_engines() {
        let t = terse_text();
        assert!(t.contains("add <project> <prompt>"));
        assert!(t.contains("done --id [--report]"));
        assert!(!t.contains("[just "), "terse must drop recipe hints");
        assert!(
            !t.contains("List open items."),
            "terse must drop descriptions"
        );
        assert!(t.contains("handoff <verb>"));
        assert!(t.contains("migrate"));
        assert!(t.contains("note <project>"));
        assert!(t.contains("rename-project --old"));
    }

    #[test]
    fn terse_engine_scopes_rename_project() {
        assert!(
            terse_engine("rename-project")
                .unwrap()
                .contains("--old <CODE>")
        );
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
