//! argv preprocessing for top-level pending-work defaults and the two implicit
//! subcommand defaults clap can't derive: bare `pw` -> `pw list`, and `pw
//! <words…>` (a non-verb lead) -> `pw route <words…>` (the word-router). Canonical
//! `pw` verbs and the `handoff`/`migrate` engines pass through untouched for
//! clap to parse.

/// pw verbs reachable as clap subcommands (incl. the hidden `route`). A
/// leading positional matching one passes through; anything else is treated as
/// router words. The route sub-verb abbreviations still fall through to `route`.
fn pw_subcommands() -> &'static [&'static str] {
    &[
        "add", "list", "ls", "check", "cancel", "reopen", "update", "resolve", "show", "session",
        "clean", "verify", "remove", "route",
    ]
}

fn is_root_engine(token: &str) -> bool {
    matches!(
        token,
        "pw" | "pending-work"
            | "handoff"
            | "migrate"
            | "note"
            | "--help"
            | "-h"
            | "help"
            | "--list"
            | "--version"
            | "-V"
            | "--terse"
    )
}

/// Whether `flag` consumes the following token as its value. Keeps a flag's value
/// out of the positional stream so the list/route decision sees only real words.
fn is_value_flag(flag: &str) -> bool {
    matches!(
        flag,
        "--config-path"
            | "--notes-dir"
            | "--repo-root"
            | "--pending-work-script"
            | "--id"
            | "--project"
            | "--prompt"
            | "--prereq"
            | "--commits"
            | "--title"
            | "--slug"
            | "--reason"
            | "--report"
            | "--append-report"
            | "--date"
            | "--section"
            | "--continue"
            | "--number"
            | "--color"
            | "--agent"
    )
}

/// Short value-flags (single-dash, consume the following token). Mirrors
/// `is_value_flag` so the implicit list/route forms don't misread the value as a word.
fn is_short_value_flag(tok: &str) -> bool {
    tok == "-n" || tok == "-a"
}

/// Inject the implicit `list`/`route` subcommand for the `pw` engine when no
/// canonical verb leads. Idempotent on input that already names a verb; a no-op
/// for `handoff`/`migrate` (clap reports a missing subcommand itself).
pub fn normalize(argv: Vec<String>) -> Vec<String> {
    if argv.is_empty() {
        return argv;
    }
    let engine_lower = argv[0].to_ascii_lowercase();
    if !is_root_engine(&engine_lower) && !argv[0].starts_with('-') {
        let mut with_default = Vec::with_capacity(argv.len() + 1);
        with_default.push("pw".to_string());
        with_default.extend(argv);
        return normalize(with_default);
    }
    let is_pw = engine_lower == "pw" || engine_lower == "pending-work";
    if !is_pw {
        return argv;
    }

    // Split flags (and their values) from true positional words.
    let mut opts: Vec<String> = Vec::new();
    let mut positionals: Vec<String> = Vec::new();
    let mut i = 1;
    while i < argv.len() {
        let tok = &argv[i];
        if tok.starts_with("--") {
            opts.push(tok.clone());
            if is_value_flag(tok)
                && let Some(val) = argv.get(i + 1)
            {
                opts.push(val.clone());
                i += 2;
                continue;
            }
            i += 1;
        } else if is_short_value_flag(tok) {
            opts.push(tok.clone());
            if let Some(val) = argv.get(i + 1) {
                opts.push(val.clone());
                i += 2;
                continue;
            }
            i += 1;
        } else {
            positionals.push(tok.clone());
            i += 1;
        }
    }

    let first_is_verb = positionals
        .first()
        .map(|w| pw_subcommands().contains(&w.as_str()))
        .unwrap_or(false);

    let mut out = vec![argv[0].clone()];
    if first_is_verb {
        // clap needs the verb before its options; emit it first.
        let mut rest = positionals.into_iter();
        out.push(rest.next().unwrap());
        out.extend(opts);
        out.extend(rest);
    } else if positionals.is_empty() {
        out.push("list".to_string());
        out.extend(opts);
    } else {
        out.push("route".to_string());
        out.extend(opts);
        out.extend(positionals);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(tokens: &[&str]) -> Vec<String> {
        normalize(tokens.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn bare_pw_defaults_to_list() {
        assert_eq!(n(&["pw"]), vec!["pw", "list"]);
    }

    #[test]
    fn session_agent_long_flag_keeps_its_value() {
        // `--agent` consumes the following token; a trailing flag must not strand the
        // value as a positional (the bug a no-trailing-flag case hides by coincidence).
        assert_eq!(
            n(&["session", "--id", "PWF-0001", "--agent", "codex", "--yes"]),
            vec![
                "pw", "session", "--id", "PWF-0001", "--agent", "codex", "--yes"
            ]
        );
    }

    #[test]
    fn session_agent_short_flag_keeps_its_value() {
        // `-a codex` must keep its value through the flag/positional split.
        assert_eq!(
            n(&["session", "PWF-0001", "-a", "codex"]),
            vec!["pw", "session", "-a", "codex", "PWF-0001"]
        );
    }

    #[test]
    fn bare_pw_verb_defaults_to_pending_work() {
        assert_eq!(
            n(&["add", "glep-shimeji", "x"]),
            vec!["pw", "add", "glep-shimeji", "x"]
        );
    }

    #[test]
    fn bare_project_defaults_to_route() {
        assert_eq!(n(&["glep-shimeji"]), vec!["pw", "route", "glep-shimeji"]);
    }

    #[test]
    fn flags_only_without_word_lists() {
        assert_eq!(n(&["pw", "--long"]), vec!["pw", "list", "--long"]);
    }

    #[test]
    fn top_level_version_flag_passes_to_clap() {
        assert_eq!(n(&["--version"]), vec!["--version"]);
    }

    #[test]
    fn leading_non_verb_word_routes() {
        assert_eq!(
            n(&["pw", "glep-shimeji", "do", "x"]),
            vec!["pw", "route", "glep-shimeji", "do", "x"]
        );
    }

    #[test]
    fn canonical_subcommand_passes_through() {
        assert_eq!(
            n(&["pw", "add", "--project", "x"]),
            vec!["pw", "add", "--project", "x"]
        );
    }

    #[test]
    fn canonical_remove_subcommand_passes_through() {
        assert_eq!(
            n(&["pw", "remove", "--id", "PWF-0001"]),
            vec!["pw", "remove", "--id", "PWF-0001"]
        );
    }

    #[test]
    fn value_flag_value_is_not_mistaken_for_a_route_word() {
        // `--config-path /foo` precedes the verb; its value must not become the
        // first positional (which would wrongly trigger the router).
        assert_eq!(
            n(&["pw", "--config-path", "/foo", "list"]),
            vec!["pw", "list", "--config-path", "/foo"]
        );
    }

    #[test]
    fn route_keeps_flags_and_words() {
        assert_eq!(
            n(&["pw", "glep", "do", "x", "--human"]),
            vec!["pw", "route", "--human", "glep", "do", "x"]
        );
    }

    #[test]
    fn route_shorthand_forwards_all_flag() {
        assert_eq!(
            n(&["pw", "glep", "--all"]),
            vec!["pw", "route", "--all", "glep"]
        );
    }

    #[test]
    fn section_value_is_consumed_with_its_flag() {
        // `--section future` precedes the positional words on `add`; `future` is the
        // flag value, not a route/positional word (PWF-0034).
        assert_eq!(
            n(&["pw", "add", "glep", "--section", "future", "do", "x"]),
            vec!["pw", "add", "--section", "future", "glep", "do", "x"]
        );
    }

    // ! PWF-0065: `show` is a canonical verb — it must pass through to clap, not be
    // misread as a route word (which would list the "show" project instead). Its id
    // is a bare positional that stays attached behind the verb.
    #[test]
    fn canonical_show_subcommand_passes_through() {
        assert_eq!(
            n(&["pw", "show", "pwf-0001"]),
            vec!["pw", "show", "pwf-0001"]
        );
    }

    #[test]
    fn canonical_reopen_subcommand_passes_through() {
        assert_eq!(
            n(&["pw", "reopen", "--id", "PWF-0001"]),
            vec!["pw", "reopen", "--id", "PWF-0001"]
        );
        // Also via the bare top-level form (no explicit `pw`).
        assert_eq!(
            n(&["reopen", "--id", "PWF-0001"]),
            vec!["pw", "reopen", "--id", "PWF-0001"]
        );
    }

    #[test]
    fn handoff_passes_through_untouched() {
        assert_eq!(
            n(&["handoff", "done", "--id", "h1"]),
            vec!["handoff", "done", "--id", "h1"]
        );
    }

    #[test]
    fn note_passes_through_untouched() {
        assert_eq!(
            n(&["note", "pwf", "add", "buy", "milk"]),
            vec!["note", "pwf", "add", "buy", "milk"]
        );
    }

    // ! PWF-0020: the short value-flag `-n` and its value must land in opts, not the
    // positional stream — else the implicit list/route forms misroute.
    #[test]
    fn bare_pw_with_number_defaults_to_list() {
        assert_eq!(n(&["pw", "-n", "5"]), vec!["pw", "list", "-n", "5"]);
    }

    #[test]
    fn number_value_is_not_mistaken_for_a_route_word() {
        assert_eq!(
            n(&["pw", "-n", "5", "glep-shimeji"]),
            vec!["pw", "route", "-n", "5", "glep-shimeji"]
        );
    }

    // ! PWF-0017: `--commits` is a value-flag; its range value must stay attached and
    // not be reordered into the positional stream behind the verb.
    #[test]
    fn commits_value_stays_with_its_flag_on_check() {
        assert_eq!(
            n(&[
                "pw",
                "check",
                "--id",
                "GLP-0001",
                "--commits",
                "a..b",
                "--commits",
                "c..d",
            ]),
            vec![
                "pw",
                "check",
                "--id",
                "GLP-0001",
                "--commits",
                "a..b",
                "--commits",
                "c..d",
            ]
        );
    }

    // ! PWF-0065: `--append-report` is a value-flag; its multi-line Markdown value
    // (which leads with `#`/`-`-style tokens, not `--`) must stay attached to the flag
    // rather than being reordered into the positional stream behind the verb.
    #[test]
    fn append_report_value_stays_with_its_flag_on_update() {
        assert_eq!(
            n(&[
                "pw",
                "update",
                "--id",
                "PWF-0003",
                "--append-report",
                "## Outcome\n\nshipped it",
            ]),
            vec![
                "pw",
                "update",
                "--id",
                "PWF-0003",
                "--append-report",
                "## Outcome\n\nshipped it",
            ]
        );
    }

    #[test]
    fn long_number_flag_routes_too() {
        assert_eq!(
            n(&["pw", "--number", "5"]),
            vec!["pw", "list", "--number", "5"]
        );
    }
}
