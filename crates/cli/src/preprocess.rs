//! Normalizes pending-work argv shapes that clap cannot derive.
//!
//! Bare non-verb words route through the hidden `route` action. Explicit verbs have
//! their positionals reordered ahead of flags. `note` gains an implicit `ls` before a
//! bare project; other commands and help tokens pass through.

/// Returns pending-work verbs recognized as clap subcommands, including hidden `route`.
fn pw_subcommands() -> &'static [&'static str] {
    &[
        "add", "list", "ls", "done", "cancel", "reopen", "update", "show", "s", "session",
        "verify", "remove", "route", "help",
    ]
}

/// Reports whether a long flag consumes the next token.
fn is_value_flag(flag: &str) -> bool {
    matches!(
        flag,
        "--repo-root"
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
            | "--status"
            | "--model"
    )
}

/// Reports whether a short flag consumes the next token.
fn is_short_value_flag(tok: &str) -> bool {
    tok == "-n" || tok == "-a" || tok == "-m"
}

/// Reports whether a flag consumes up to two list-order values.
fn is_order_flag(tok: &str) -> bool {
    tok == "--order" || tok == "-o"
}

/// Reports whether a pending-work verb accepts a compact split ID.
fn is_id_facing_verb(verb: &str) -> bool {
    matches!(
        verb,
        "done" | "cancel" | "reopen" | "update" | "show" | "s" | "session" | "verify" | "remove"
    )
}

/// Returns note verbs recognized as clap subcommands.
fn note_subcommands() -> &'static [&'static str] {
    &["list", "ls", "add", "remove", "update", "help"]
}

/// Injects `ls` before a bare `note <project>` so an omitted verb lists.
fn normalize_note(argv: Vec<String>) -> Vec<String> {
    let mut i = 1;
    while i < argv.len() {
        let tok = argv[i].as_str();
        if tok.starts_with("--") {
            i += if is_value_flag(tok) { 2 } else { 1 };
        } else if tok.starts_with('-') {
            i += 1;
        } else if note_subcommands().contains(&tok) {
            return argv;
        } else {
            let mut out = argv;
            out.insert(i, "ls".to_string());
            return out;
        }
    }
    argv
}

/// Reports whether a token is the 2-4 letter code half of a split ID.
fn is_code_token(tok: &str) -> bool {
    (2..=4).contains(&tok.len()) && tok.chars().all(|c| c.is_ascii_alphabetic())
}

/// Reports whether a token is the numeric half of a split ID.
fn is_number_token(tok: &str) -> bool {
    !tok.is_empty() && tok.chars().all(|c| c.is_ascii_digit())
}

/// Injects `route` for bare words and places a verb's positional values before its options.
/// Other commands, help tokens, and a leading option pass through unchanged.
pub fn normalize(argv: Vec<String>) -> Vec<String> {
    if argv.is_empty() || argv[0].starts_with('-') {
        return argv;
    }
    if argv[0].eq_ignore_ascii_case("note") {
        return normalize_note(argv);
    }

    let mut opts: Vec<String> = Vec::new();
    let mut positionals: Vec<String> = Vec::new();
    let mut i = 0;
    while i < argv.len() {
        let tok = &argv[i];
        if is_order_flag(tok) {
            opts.push(tok.clone());
            i += 1;
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
        let mut rest = positionals;
        let verb = rest.remove(0);
        // Collapse only `<verb> <code> <digits>` for ID-facing verbs; extra words block it.
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
        assert_eq!(
            n(&["session", "--id", "PWF-0001", "--model", "fable", "--yes"]),
            vec!["session", "--id", "PWF-0001", "--model", "fable", "--yes"]
        );
    }

    #[test]
    fn session_model_short_flag_keeps_its_value() {
        assert_eq!(
            n(&["session", "PWF-0001", "-m", "fable"]),
            vec!["session", "-m", "fable", "PWF-0001"]
        );
    }

    #[test]
    fn session_append_short_flag_keeps_its_value() {
        assert_eq!(
            n(&["session", "PWF-0001", "-a", "more context"]),
            vec!["session", "-a", "more context", "PWF-0001"]
        );
    }

    #[test]
    fn bare_pw_verb_defaults_to_pending_work() {
        assert_eq!(n(&["add", "foo-bar", "x"]), vec!["add", "foo-bar", "x"]);
    }

    #[test]
    fn bare_project_defaults_to_route() {
        assert_eq!(n(&["foo-bar"]), vec!["route", "foo-bar"]);
    }

    #[test]
    fn top_level_version_flag_passes_to_clap() {
        assert_eq!(n(&["--version"]), vec!["--version"]);
    }

    #[test]
    fn leading_non_verb_word_routes() {
        assert_eq!(
            n(&["foo-bar", "do", "x"]),
            vec!["route", "foo-bar", "do", "x"]
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
            n(&["foo", "do", "x", "--human"]),
            vec!["route", "--human", "foo", "do", "x"]
        );
    }

    #[test]
    fn route_shorthand_forwards_all_flag() {
        assert_eq!(n(&["foo", "--all"]), vec!["route", "--all", "foo"]);
    }

    #[test]
    fn project_route_keeps_status_value_with_its_flag() {
        assert_eq!(
            n(&["pwf", "--status", "done"]),
            vec!["route", "--status", "done", "pwf"]
        );
    }

    #[test]
    fn section_value_is_consumed_with_its_flag() {
        assert_eq!(
            n(&["add", "foo", "--section", "future", "do", "x"]),
            vec!["add", "--section", "future", "foo", "do", "x"]
        );
    }

    #[test]
    fn effort_value_stays_with_its_flag_on_add() {
        assert_eq!(
            n(&["add", "foo", "--effort", "high", "do", "x"]),
            vec!["add", "--effort", "high", "foo", "do", "x"]
        );
    }

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

    #[test]
    fn canonical_show_subcommand_passes_through() {
        assert_eq!(n(&["show", "pwf-0001"]), vec!["show", "pwf-0001"]);
    }

    #[test]
    fn show_alias_s_is_a_verb_not_a_project_word() {
        assert_eq!(n(&["s", "pwf-0127"]), vec!["s", "pwf-0127"]);
    }

    #[test]
    fn show_alias_s_collapses_split_id() {
        assert_eq!(n(&["s", "pwf", "127"]), vec!["s", "pwf-127"]);
    }

    #[test]
    fn canonical_reopen_subcommand_passes_through() {
        assert_eq!(
            n(&["reopen", "--id", "PWF-0001"]),
            vec!["reopen", "--id", "PWF-0001"]
        );
    }

    #[test]
    fn note_verb_first_passes_through_untouched() {
        assert_eq!(
            n(&["note", "add", "pwf", "buy", "milk"]),
            vec!["note", "add", "pwf", "buy", "milk"]
        );
    }

    #[test]
    fn note_bare_project_gains_implicit_ls() {
        assert_eq!(n(&["note", "pwf"]), vec!["note", "ls", "pwf"]);
    }

    #[test]
    fn note_without_positionals_passes_through_untouched() {
        assert_eq!(n(&["note", "--help"]), vec!["note", "--help"]);
    }

    #[test]
    fn commits_value_stays_with_its_flag_on_done() {
        assert_eq!(
            n(&[
                "done",
                "--id",
                "FOO-0001",
                "--commits",
                "a..b",
                "--commits",
                "c..d",
            ]),
            vec![
                "done",
                "--id",
                "FOO-0001",
                "--commits",
                "a..b",
                "--commits",
                "c..d",
            ]
        );
    }

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
        assert_eq!(n(&["show", "wne", "48"]), vec!["show", "wne-48"]);
    }

    #[test]
    fn split_id_form_tolerates_trailing_short_flags() {
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
        assert_eq!(n(&["add", "cfg", "57"]), vec!["add", "cfg", "57"]);
    }
}
