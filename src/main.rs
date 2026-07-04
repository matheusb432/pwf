use pwf::{command, engines, help};

const DEPRECATED_PW_PREFIX_WARNING: &str =
    "If you need help for pending-work verbs, use `pwf <verb> --help`.";

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if let Some(code) = pwf::codex_thread_title::maybe_run(&argv) {
        std::process::exit(code);
    }
    if let Some(prefix) = retired_pending_work_prefix(&argv) {
        print_retired_pending_work_prefix_error(prefix, &argv);
        std::process::exit(1);
    }

    if argv.iter().any(|a| help::is_terse(a)) {
        print_terse_help(&argv);
        return;
    }
    let argv = normalize_rich_help_aliases(argv);

    // Canonical path: subcommand-default injection -> clap -> typed dispatch.
    match command::parse_command_argv(argv) {
        Ok(parsed) => match run_parsed(parsed) {
            Ok(out) => {
                if !out.is_empty() {
                    println!("{out}");
                }
            }
            Err(e) => {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        },
        // clap renders help/version/parse errors with the right exit codes.
        Err(e) => exit_with_clap_error(&e),
    }
}

/// Print a clap-rendered help/version/parse error and exit with clap's code.
fn exit_with_clap_error(e: &clap::Error) -> ! {
    let rendered = e.render().to_string();
    if e.use_stderr() {
        eprint!("{rendered}");
    } else {
        print!("{rendered}");
    }
    std::process::exit(e.exit_code());
}

fn run_parsed(parsed: command::ParsedCommand) -> Result<String, String> {
    match parsed {
        command::ParsedCommand::PendingWork(command) => engines::pending_work::run(&command),
        command::ParsedCommand::Handoff(args) => engines::handoff::run(&args),
        command::ParsedCommand::Migrate(args) => engines::migrate::run(&args),
        command::ParsedCommand::Note(command) => pwf_note::run(&command),
    }
}

fn normalize_rich_help_aliases(argv: Vec<String>) -> Vec<String> {
    if argv.is_empty() || argv.first().is_some_and(|arg| arg == "--list") {
        return vec!["--help".to_string()];
    }
    argv
}

fn print_terse_help(argv: &[String]) {
    let scope = argv
        .iter()
        .find(|arg| !help::is_terse(arg) && !help::is_help_token(arg));
    let text = scope
        .and_then(|arg| help::terse_engine(arg).or_else(|| help::terse_verb(arg)))
        .unwrap_or_else(help::terse_text);
    println!("{text}");
}

fn retired_pending_work_prefix(argv: &[String]) -> Option<&str> {
    match argv.first().map(String::as_str) {
        Some(prefix) if prefix.eq_ignore_ascii_case("pw") => Some("pw"),
        Some(prefix) if prefix.eq_ignore_ascii_case("pending-work") => Some("pending-work"),
        _ => None,
    }
}

fn print_retired_pending_work_prefix_error(prefix: &str, argv: &[String]) {
    let replacement = pending_work_prefix_replacement(argv);
    eprintln!("warning: `pwf {prefix} ...` is deprecated and no longer supported.");
    eprintln!(
        "error: use `{replacement}` instead; `pwf {prefix} ...` is a retired compatibility surface."
    );
    eprintln!("{DEPRECATED_PW_PREFIX_WARNING}");
}

fn pending_work_prefix_replacement(argv: &[String]) -> String {
    match argv.get(1).map(String::as_str) {
        Some(token) if help::is_help_token(token) || help::is_terse(token) => {
            "pwf --help".to_string()
        }
        Some(token) if !token.starts_with('-') => format!("pwf {token}"),
        _ => "pwf".to_string(),
    }
}
