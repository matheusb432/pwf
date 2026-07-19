use pwf::{command, engines};

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let codex_thread_title_protocol = pwf::codex_thread_title::CodexThreadTitleProtocol {
        launch_command: pwf_infra::session::codex::LAUNCH_COMMAND,
        worker_command: pwf_infra::session::codex::WORKER_COMMAND,
        binary: pwf_infra::session::codex::BINARY,
        title_flag: pwf_infra::session::codex::TITLE_FLAG,
        cwd_flag: pwf_infra::session::codex::CWD_FLAG,
        since_flag: pwf_infra::session::codex::SINCE_FLAG,
        argument_separator: pwf_infra::session::codex::ARG_SEPARATOR,
    };
    if let Some(code) = pwf::codex_thread_title::maybe_run(&argv, &codex_thread_title_protocol) {
        std::process::exit(code);
    }

    let argv = normalize_rich_help_aliases(argv);

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
        Err(e) => exit_with_clap_error(&e),
    }
}

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
        command::ParsedCommand::Note(command) => pwf_note::run(&command),
    }
}

fn normalize_rich_help_aliases(argv: Vec<String>) -> Vec<String> {
    if argv.is_empty() || argv.first().is_some_and(|arg| arg == "--list") {
        return vec!["--help".to_string()];
    }
    argv
}
