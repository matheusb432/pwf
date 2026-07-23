use std::io::Write;

use pwf::{command, engines};

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let argv = normalize_rich_help_aliases(argv);

    match command::parse_argv(argv) {
        Ok(parsed) => match run(parsed) {
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
        Err(e) => exit_with_clap_error(&e),
    }
}

fn exit_with_clap_error(e: &clap::Error) -> ! {
    let rendered = e.render().ansi().to_string();
    if e.use_stderr() {
        let _ = anstream::stderr().write_all(rendered.as_bytes());
    } else {
        let _ = anstream::stdout().write_all(rendered.as_bytes());
    }
    std::process::exit(e.exit_code());
}

fn run(parsed: command::Cli) -> Result<String, String> {
    match parsed.engine {
        command::Engine::PendingWork(command) => {
            engines::pending_work::run(&command, pwf::console::Console::from_terminal())
        }
        command::Engine::Handoff { command } => engines::handoff::run(&command),
        command::Engine::Note(arguments) => engines::note::run(&arguments),
    }
}

fn normalize_rich_help_aliases(argv: Vec<String>) -> Vec<String> {
    if argv.is_empty() || argv.first().is_some_and(|arg| arg == "--list") {
        return vec!["--help".to_string()];
    }
    argv
}
