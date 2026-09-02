//! Normalizes task argv shapes that clap cannot derive.
//!
//! Bare non-command words route through the hidden `route` action. Task shims gain the
//! canonical `task` noun, explicit commands place positionals after flags, and `note`
//! gains an implicit `ls` before a bare project.

/// Returns task verbs accepted with or without the canonical `task` noun.
fn task_subcommands() -> &'static [&'static str] {
    &[
        "add", "list", "ls", "done", "cancel", "reopen", "edit", "get", "g", "dag", "remove",
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
            | "--blocked-by"
            | "--add-blocked-by"
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
            | "--priority"
            | "--status"
            | "--depth"
            | "--mode"
            | "--with"
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
        "done" | "cancel" | "reopen" | "edit" | "get" | "g" | "dag" | "session" | "remove"
    )
}

/// Returns note verbs recognized as clap subcommands.
fn note_subcommands() -> &'static [&'static str] {
    &["list", "ls", "add", "remove", "edit", "help"]
}

/// Returns note verbs retired from the implicit project-list fallback.
fn note_reserved_subcommands() -> &'static [&'static str] {
    &["get", "update"]
}

/// Injects `ls` before a bare `note <project>` so an omitted verb lists.
fn normalize_note(argv: Vec<String>) -> Vec<String> {
    let mut i = 1;
    while i < argv.len() {
        let tok = argv[i].as_str();
        if tok.starts_with("--") {
            i += long_option_width(tok);
            continue;
        }
        if tok.starts_with('-') {
            i += 1;
            continue;
        }
        if note_subcommands().contains(&tok) || note_reserved_subcommands().contains(&tok) {
            return argv;
        }
        let mut out = argv;
        out.insert(i, "ls".to_string());
        return out;
    }
    argv
}

fn long_option_width(option: &str) -> usize {
    if is_value_flag(option) { 2 } else { 1 }
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
            i += push_order_option(argv, i, &mut opts);
            continue;
        }
        if tok.starts_with("--") {
            i += push_long_option(argv, i, &mut opts);
            continue;
        }
        if is_short_value_flag(tok) {
            i += push_short_value_option(argv, i, &mut opts);
            continue;
        }
        positionals.push(tok.clone());
        i += 1;
    }

    (opts, positionals)
}

fn push_order_option(argv: &[String], index: usize, options: &mut Vec<String>) -> usize {
    options.push(argv[index].clone());
    let values = argv
        .iter()
        .skip(index + 1)
        .take(2)
        .take_while(|value| !value.starts_with('-'))
        .cloned()
        .collect::<Vec<_>>();
    let consumed = values.len() + 1;
    options.extend(values);
    consumed
}

fn push_long_option(argv: &[String], index: usize, options: &mut Vec<String>) -> usize {
    let option = &argv[index];
    options.push(option.clone());
    if is_value_flag(option)
        && let Some(value) = argv.get(index + 1)
    {
        options.push(value.clone());
        return 2;
    }
    1
}

fn push_short_value_option(argv: &[String], index: usize, options: &mut Vec<String>) -> usize {
    options.push(argv[index].clone());
    if let Some(value) = argv.get(index + 1) {
        options.push(value.clone());
        return 2;
    }
    1
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
