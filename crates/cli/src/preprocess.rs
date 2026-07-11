//! argv preprocessing for the two implicit pending-work defaults clap can't
//! derive: bare `pwf <words…>` (a non-verb lead) -> `pwf route <words…>` (the
//! hidden word-router), and a flags-only positional gap on an explicit verb ->
//! that verb's own flag/value reordering. Pending-work verbs (`add`, `list`, …)
//! are flattened top-level clap subcommands (`command.rs`) — there is no
//! separate `pw` engine token to inject or detect here. `handoff`/`migrate`/
//! `note` and help/version tokens pass through untouched for clap (or
//! `help.rs`) to handle; `pwf pw …`/`pwf pending-work …` themselves are a
//! retired compatibility surface `main::retired_pending_work_prefix` rejects
//! before this module ever runs.

/// pw verbs reachable as clap subcommands (incl. the hidden `route`). A
/// leading positional matching one passes through; anything else is treated as
/// router words. The route sub-verb abbreviations still fall through to `route`.
fn pw_subcommands() -> &'static [&'static str] {
    &[
        "add", "list", "ls", "done", "cancel", "reopen", "update", "resolve", "show", "session",
        "clean", "verify", "remove", "route",
    ]
}

/// Non-pending-work top-level tokens: the dedicated engines and help/version
/// requests. These pass through untouched — clap or `help.rs` handles them.
fn is_other_root_token(token: &str) -> bool {
    matches!(
        token,
        "handoff"
            | "migrate"
            | "note"
            | "rename-project"
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
            | "--tag"
            | "--commits"
            | "--title"
            | "--slug"
            | "--report"
            | "--append-report"
            | "--append"
            | "--date"
            | "--section"
            | "--continue"
            | "--number"
            | "--color"
            | "--agent"
            | "--effort"
            | "--model"
    )
}

/// Short value-flags (single-dash, consume the following token). Mirrors
/// `is_value_flag` so the implicit route form doesn't misread the value as a word.
fn is_short_value_flag(tok: &str) -> bool {
    tok == "-n" || tok == "-a" || tok == "-m"
}

/// `list`'s `-o`/`--order`: unlike the fixed-arity value-flags above, it takes
/// 0-2 following values (clap's `num_args = 0..=2`), so it needs its own
/// bounded consumption rather than the unconditional single-value grab.
fn is_order_flag(tok: &str) -> bool {
    tok == "--order" || tok == "-o"
}

/// Pending-work verbs that take a task id (and so accept the compact split form).
/// A strict subset of `pw_subcommands` — excludes `add`/`list`/`ls`/`clean`/`route`,
/// which take a project/word positional, not an id.
fn is_id_facing_verb(verb: &str) -> bool {
    matches!(
        verb,
        "done"
            | "cancel"
            | "reopen"
            | "update"
            | "resolve"
            | "show"
            | "session"
            | "verify"
            | "remove"
    )
}

/// A bare 2–4 letter project code (the prefix half of the compact split id form).
fn is_code_token(tok: &str) -> bool {
    (2..=4).contains(&tok.len()) && tok.chars().all(|c| c.is_ascii_alphabetic())
}

/// An all-digits token (the number half of the compact split id form).
fn is_number_token(tok: &str) -> bool {
    !tok.is_empty() && tok.chars().all(|c| c.is_ascii_digit())
}

/// Inject the implicit `route` verb when no canonical pending-work verb leads,
/// and reorder a canonical verb's positional words ahead of its flags/values
/// (clap's own trailing-`Vec<String>` args expect the words contiguous). A
/// no-op for `handoff`/`migrate`/`note`/help/version — clap or `help.rs`
/// handles those directly — and for a leading flag, which clap rejects itself
/// (there's no top-level flag without a subcommand first).
pub fn normalize(argv: Vec<String>) -> Vec<String> {
    if argv.is_empty() || argv[0].starts_with('-') {
        return argv;
    }
    if is_other_root_token(&argv[0].to_ascii_lowercase()) {
        return argv;
    }

    // Split flags (and their values) from true positional words.
    let mut opts: Vec<String> = Vec::new();
    let mut positionals: Vec<String> = Vec::new();
    let mut i = 0;
    while i < argv.len() {
        let tok = &argv[i];
        if is_order_flag(tok) {
            opts.push(tok.clone());
            i += 1;
            // Up to 2 bare-word values (clap's own value_parser rejects an
            // invalid one later); stop at the first token that looks like a
            // flag, or after 2, whichever comes first.
            for _ in 0..2 {
                match argv.get(i) {
                    Some(val) if !val.starts_with('-') => {
                        opts.push(val.clone());
                        i += 1;
                    }
                    _ => break,
                }
            }
        } else if tok.starts_with("--") {
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
        .is_some_and(|w| pw_subcommands().contains(&w.as_str()));

    let mut out = Vec::with_capacity(argv.len());
    if first_is_verb {
        // clap needs the verb before its options; emit it first.
        let mut rest = positionals;
        let verb = rest.remove(0);
        // Strict compact split id form: `<verb> <code> <digits> [flags…]` ->
        // `<verb> <code>-<digits> [flags…]`, for id-facing verbs only. Trailing
        // bare short flags (e.g. `-i`) are tolerated; anything else blocks the join.
        if is_id_facing_verb(&verb)
            && rest.len() >= 2
            && is_code_token(&rest[0])
            && is_number_token(&rest[1])
            && rest[2..].iter().all(|t| t.starts_with('-'))
        {
            let joined = format!("{}-{}", rest[0], rest[1]);
            let mut new_rest = vec![joined];
            new_rest.extend_from_slice(&rest[2..]);
            rest = new_rest;
        }
        out.push(verb);
        out.extend(opts);
        out.extend(rest);
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
        normalize(
            tokens
                .iter()
                .map(std::string::ToString::to_string)
                .collect(),
        )
    }

    #[test]
    fn session_agent_long_flag_keeps_its_value() {
        // `--agent` consumes the following token; a trailing flag must not strand the
        // value as a positional (the bug a no-trailing-flag case hides by coincidence).
        assert_eq!(
            n(&["session", "--id", "PWF-0001", "--agent", "codex", "--yes"]),
            vec!["session", "--id", "PWF-0001", "--agent", "codex", "--yes"]
        );
    }

    #[test]
    fn tag_value_stays_with_its_flag_when_it_begins_with_punctuation() {
        assert_eq!(
            n(&["add", "pwf", "do", "x", "--tag", "_sqlite", "--human"]),
            vec!["add", "--tag", "_sqlite", "--human", "pwf", "do", "x"]
        );
    }

    #[test]
    fn session_model_long_flag_keeps_its_value() {
        // Same pitfall as `--agent`: an unregistered value-flag lets its value get
        // stranded as a bare positional and reordered after a trailing flag.
        assert_eq!(
            n(&["session", "--id", "PWF-0001", "--model", "fable", "--yes"]),
            vec!["session", "--id", "PWF-0001", "--model", "fable", "--yes"]
        );
    }

    #[test]
    fn session_model_short_flag_keeps_its_value() {
        // PWF-0102: `-m` is `--model`'s shorthand. Like the long form it must keep
        // its value out of the positional stream through the flag/positional split.
        assert_eq!(
            n(&["session", "PWF-0001", "-m", "fable"]),
            vec!["session", "-m", "fable", "PWF-0001"]
        );
    }

    #[test]
    fn session_append_short_flag_keeps_its_value() {
        // PWF-0088: session's `-a` is `--append` (the `--agent` shorthand was
        // dropped to free it). `-a "more context"` must keep its value through
        // the flag/positional split.
        assert_eq!(
            n(&["session", "PWF-0001", "-a", "more context"]),
            vec!["session", "-a", "more context", "PWF-0001"]
        );
    }

    #[test]
    fn bare_pw_verb_defaults_to_pending_work() {
        assert_eq!(
            n(&["add", "glep-shimeji", "x"]),
            vec!["add", "glep-shimeji", "x"]
        );
    }

    #[test]
    fn bare_project_defaults_to_route() {
        assert_eq!(n(&["glep-shimeji"]), vec!["route", "glep-shimeji"]);
    }

    #[test]
    fn top_level_version_flag_passes_to_clap() {
        assert_eq!(n(&["--version"]), vec!["--version"]);
    }

    #[test]
    fn leading_non_verb_word_routes() {
        assert_eq!(
            n(&["glep-shimeji", "do", "x"]),
            vec!["route", "glep-shimeji", "do", "x"]
        );
    }

    #[test]
    fn canonical_subcommand_passes_through() {
        assert_eq!(n(&["add", "--project", "x"]), vec!["add", "--project", "x"]);
    }

    #[test]
    fn canonical_remove_subcommand_passes_through() {
        assert_eq!(
            n(&["remove", "--id", "PWF-0001"]),
            vec!["remove", "--id", "PWF-0001"]
        );
    }

    #[test]
    fn route_keeps_flags_and_words() {
        assert_eq!(
            n(&["glep", "do", "x", "--human"]),
            vec!["route", "--human", "glep", "do", "x"]
        );
    }

    #[test]
    fn route_shorthand_forwards_all_flag() {
        assert_eq!(n(&["glep", "--all"]), vec!["route", "--all", "glep"]);
    }

    #[test]
    fn section_value_is_consumed_with_its_flag() {
        // `--section future` precedes the positional words on `add`; `future` is the
        // flag value, not a route/positional word (PWF-0034).
        assert_eq!(
            n(&["add", "glep", "--section", "future", "do", "x"]),
            vec!["add", "--section", "future", "glep", "do", "x"]
        );
    }

    // ! PWF-0091: `--effort` is a value-flag on `add`; its numeric value must stay
    // attached to the flag rather than being reordered into the positional stream
    // behind the verb.
    #[test]
    fn effort_value_stays_with_its_flag_on_add() {
        assert_eq!(
            n(&["add", "glep", "--effort", "3", "do", "x"]),
            vec!["add", "--effort", "3", "glep", "do", "x"]
        );
    }

    // ! PWF-0096: `--order` takes 0-2 bare-word values (created|id|asc|desc); they
    // must stay attached to the flag, not be reordered as positional route words
    // behind a trailing flag like `--long`.
    #[test]
    fn order_values_stay_with_its_flag_on_list() {
        assert_eq!(
            n(&["list", "--order", "created", "desc", "--long"]),
            vec!["list", "--order", "created", "desc", "--long"]
        );
    }

    #[test]
    fn order_single_value_stays_with_its_flag_on_list() {
        assert_eq!(
            n(&["list", "--order", "id", "--long"]),
            vec!["list", "--order", "id", "--long"]
        );
    }

    #[test]
    fn bare_order_flag_consumes_no_values() {
        assert_eq!(
            n(&["list", "--order", "--long"]),
            vec!["list", "--order", "--long"]
        );
    }

    #[test]
    fn short_order_flag_values_stay_attached() {
        assert_eq!(
            n(&["list", "-o", "id", "asc", "--long"]),
            vec!["list", "-o", "id", "asc", "--long"]
        );
    }

    // ! PWF-0065: `show` is a canonical verb — it must pass through to clap, not be
    // misread as a route word (which would list the "show" project instead). Its id
    // is a bare positional that stays attached behind the verb.
    #[test]
    fn canonical_show_subcommand_passes_through() {
        assert_eq!(n(&["show", "pwf-0001"]), vec!["show", "pwf-0001"]);
    }

    #[test]
    fn canonical_reopen_subcommand_passes_through() {
        assert_eq!(
            n(&["reopen", "--id", "PWF-0001"]),
            vec!["reopen", "--id", "PWF-0001"]
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
    fn rename_project_passes_through_untouched() {
        assert_eq!(
            n(&[
                "rename-project",
                "--old",
                "CFG",
                "--new",
                "ARC",
                "--new-path",
                "self/repository"
            ]),
            vec![
                "rename-project",
                "--old",
                "CFG",
                "--new",
                "ARC",
                "--new-path",
                "self/repository"
            ]
        );
    }

    #[test]
    fn note_passes_through_untouched() {
        assert_eq!(
            n(&["note", "pwf", "add", "buy", "milk"]),
            vec!["note", "pwf", "add", "buy", "milk"]
        );
    }

    // ! PWF-0017: `--commits` is a value-flag; its range value must stay attached and
    // not be reordered into the positional stream behind the verb.
    #[test]
    fn commits_value_stays_with_its_flag_on_done() {
        assert_eq!(
            n(&[
                "done",
                "--id",
                "GLP-0001",
                "--commits",
                "a..b",
                "--commits",
                "c..d",
            ]),
            vec![
                "done",
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
                "update",
                "--id",
                "PWF-0003",
                "--append-report",
                "## Outcome\n\nshipped it",
            ]),
            vec![
                "update",
                "--id",
                "PWF-0003",
                "--append-report",
                "## Outcome\n\nshipped it",
            ]
        );
    }

    // ! PWF-0090: `--append`/`-a` is a value-flag whose lane-syntax value leads with
    // `/`-style tokens; it must stay attached to its flag through the same split.
    #[test]
    fn append_long_flag_value_stays_with_its_flag_on_update() {
        assert_eq!(
            n(&[
                "update",
                "--id",
                "PWF-0090",
                "--append",
                "another goal /c new context",
            ]),
            vec![
                "update",
                "--id",
                "PWF-0090",
                "--append",
                "another goal /c new context",
            ]
        );
    }

    #[test]
    fn append_short_flag_value_stays_with_its_flag_on_update() {
        assert_eq!(
            n(&["update", "PWF-0090", "-a", "more work"]),
            vec!["update", "-a", "more work", "PWF-0090"]
        );
    }

    #[test]
    fn split_id_form_collapses_for_id_facing_verb() {
        assert_eq!(n(&["done", "cfg", "57"]), vec!["done", "cfg-57"]);
        assert_eq!(n(&["resolve", "wne", "48"]), vec!["resolve", "wne-48"]);
    }

    #[test]
    fn split_id_form_tolerates_trailing_short_flags() {
        // Bare short flags (`-i`) land in the positional stream; the join must
        // still fire and keep the flag.
        assert_eq!(
            n(&["session", "wne", "48", "-i"]),
            vec!["session", "wne-48", "-i"]
        );
    }

    #[test]
    fn split_id_form_does_not_collapse_when_second_token_is_not_digits() {
        assert_eq!(n(&["done", "cfg", "five"]), vec!["done", "cfg", "five"]);
    }

    #[test]
    fn split_id_form_does_not_collapse_three_words() {
        assert_eq!(
            n(&["done", "cfg", "57", "99"]),
            vec!["done", "cfg", "57", "99"]
        );
    }

    #[test]
    fn split_id_form_does_not_collapse_for_non_id_verb() {
        // `add cfg 57` = add to project cfg with prompt "57"; must not become an id.
        assert_eq!(n(&["add", "cfg", "57"]), vec!["add", "cfg", "57"]);
    }
}
