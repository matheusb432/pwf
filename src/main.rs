use clap::CommandFactory;
use pwf::{command, engines, help};

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();

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
        // ? render_help (compact) matches cfgtool's format; render_long_help
        // ? blows every flag onto its own next line (pwf docs are one-liners).
        print!("{}", command::Cli::command().render_help());
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
    // `pending-work` is a hidden alias of the `pw` subcommand.
    let name = if engine.eq_ignore_ascii_case("pending-work") {
        "pw"
    } else {
        engine
    };
    match cmd.find_subcommand_mut(name) {
        Some(sub) => print!("{}", sub.render_help()),
        None => print!("{}", cmd.render_help()),
    }
}
