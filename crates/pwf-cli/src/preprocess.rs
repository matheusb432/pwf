//! Normalizes task argv shapes that clap cannot derive.
//!
//! Bare non-command words route through the hidden `route` action. Task shims gain the
//! canonical `task` noun, explicit commands place positionals after flags, and `note`
//! gains an implicit `ls` before a bare project.

/// Returns task verbs accepted with or without the canonical `task` noun.
fn task_subcommands() -> &'static [&'static str] {
    &[
        "add", "list", "ls", "done", "cancel", "reopen", "edit", "show", "s", "remove",
    ]
}

/// Returns non-task commands owned by the task process root.
fn task_root_subcommands() -> &'static [&'static str] {
    &["session", "route", "help"]
}

/// Reports whether a long flag consumes the next token.
fn is_value_flag(flag: &str) -> bool {
    matches!(
        flag,
        "--id"
            | "--project"
            | "--prompt"
            | "--prereq"
            | "--add-prereq"
            | "--tag"
            | "--add-tag"
            | "--commits"
            | "--title"
            | "--goal"
            | "--context"
            | "--constraint"
            | "--done-when"
            | "--add-goal"
            | "--add-context"
            | "--add-constraint"
            | "--add-done-when"
            | "--slug"
            | "--report"
            | "--append"
            | "--date"
            | "--section"
            | "--number"
            | "--color"
            | "--agent"
            | "--effort"
            | "--status"
            | "--model"
            | "--push-prompt"
    )
}

/// Reports whether a short flag consumes the next token.
fn is_short_value_flag(tok: &str) -> bool {
    tok == "-n" || tok == "-a" || tok == "-m" || tok == "-p"
}

/// Reports whether a flag consumes up to two list-order values.
fn is_order_flag(tok: &str) -> bool {
    tok == "--order" || tok == "-o"
}

/// Reports whether a task verb accepts a compact split ID.
fn is_id_facing_verb(verb: &str) -> bool {
    matches!(
        verb,
        "done" | "cancel" | "reopen" | "edit" | "show" | "s" | "session" | "remove"
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
    if argv[0].eq_ignore_ascii_case("task") {
        let mut out = vec!["task".to_string()];
        out.extend(normalize_explicit_command(&argv[1..]));
        return out;
    }

    normalize_root(&argv)
}

fn normalize_root(argv: &[String]) -> Vec<String> {
    let (opts, mut positionals) = split_options(argv);
    let first = positionals.first().map(String::as_str);

    if first.is_some_and(|word| task_subcommands().contains(&word)) {
        let verb = positionals.remove(0);
        let mut out = vec!["task".to_string()];
        out.extend(normalize_command(verb, opts, positionals));
        return out;
    }

    if first.is_some_and(|word| task_root_subcommands().contains(&word)) {
        let verb = positionals.remove(0);
        return normalize_command(verb, opts, positionals);
    }

    let mut out = Vec::with_capacity(opts.len() + positionals.len() + 1);
    out.push("route".to_string());
    out.extend(opts);
    out.extend(positionals);
    out
}

fn normalize_explicit_command(argv: &[String]) -> Vec<String> {
    if argv.is_empty() || argv[0].starts_with('-') {
        return argv.to_vec();
    }

    let (opts, mut positionals) = split_options(argv);
    let verb = positionals.remove(0);
    normalize_command(verb, opts, positionals)
}

fn split_options(argv: &[String]) -> (Vec<String>, Vec<String>) {
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

    (opts, positionals)
}

fn normalize_command(verb: String, opts: Vec<String>, mut positionals: Vec<String>) -> Vec<String> {
    if is_id_facing_verb(&verb)
        && positionals.len() >= 2
        && is_code_token(&positionals[0])
        && is_number_token(&positionals[1])
        && positionals[2..].iter().all(|token| token.starts_with('-'))
    {
        let joined = format!("{}-{}", positionals[0], positionals[1]);
        let mut normalized = vec![joined];
        normalized.extend_from_slice(&positionals[2..]);
        positionals = normalized;
    }

    let mut out = Vec::with_capacity(opts.len() + positionals.len() + 1);
    out.push(verb);
    out.extend(opts);
    out.extend(positionals);
    out
}
