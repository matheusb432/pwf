use clap::CommandFactory;
use pwf::{command, engines, help};

const DEPRECATED_PW_PREFIX_WARNING: &str =
    "If you need help for pending-work verbs, use `pwf <verb> --help`.";

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if let Some(prefix) = retired_pending_work_prefix(&argv) {
        print_retired_pending_work_prefix_error(prefix, &argv);
        std::process::exit(1);
    }

    // Custom help surfaces clap does not derive: `--terse` (token-lean agent
    // help), `--list` (alias for `--help`), and bare `help`. Handle before clap.
    let terse = argv.iter().any(|a| help::is_terse(a));
    if argv.first().map(|a| help::help_request(a)).unwrap_or(true) {
        print_top_help(terse);
        return;
    }
    if argv.get(1).map(|a| help::help_request(a)).unwrap_or(false) {
        print_engine_help(&argv[0], terse);
        return;
    }

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
        Err(e) => e.exit(),
    }
}

fn run_parsed(parsed: command::ParsedCommand) -> Result<String, String> {
    match parsed {
        command::ParsedCommand::PendingWork(command) => engines::pending_work::run(&command),
        command::ParsedCommand::Handoff(args) => engines::handoff::run(&args),
        command::ParsedCommand::Migrate(args) => engines::migrate::run(&args),
    }
}

fn print_top_help(terse: bool) {
    if terse {
        println!("{}", help::terse_text());
    } else {
        print!("{}", help::rich_top_help());
    }
}

fn print_engine_help(engine: &str, terse: bool) {
    if terse {
        println!(
            "{}",
            help::terse_engine(engine).unwrap_or_else(help::terse_text)
        );
        return;
    }
    let mut cmd = command::Cli::command();
    match cmd.find_subcommand_mut(engine) {
        Some(sub) => print!("{}", sub.render_help()),
        None => match cmd
            .find_subcommand_mut("pw")
            .and_then(|pw| pw.find_subcommand_mut(engine))
        {
            Some(sub) => print!("{}", sub.render_help()),
            None => print!("{}", cmd.render_help()),
        },
    }
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
        Some(token) if help::help_request(token) => "pwf --help".to_string(),
        Some(token) if !token.starts_with('-') => format!("pwf {token}"),
        _ => "pwf".to_string(),
    }
}
