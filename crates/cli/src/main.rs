use pwf::{command, engines, help};

const DEPRECATED_PW_PREFIX_WARNING: &str =
    "If you need help for pending-work verbs, use `pwf <verb> --help`.";

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
    if let Some(prefix) = retired_pending_work_prefix(&argv) {
        print_retired_pending_work_prefix_error(prefix, &argv);
        std::process::exit(1);
    }
    if let Some(hint) = retired_handoff_verb(&argv) {
        eprintln!("error: {hint}");
        std::process::exit(1);
    }

    if argv.iter().any(|a| help::is_terse(a)) {
        print_terse_help(&argv);
        return;
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
        command::ParsedCommand::Migrate(args) => engines::migrate::run(&args),
        command::ParsedCommand::Note(command) => pwf_note::run(&command),
        command::ParsedCommand::RenameProject(args) => engines::rename_project::run(&args),
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

/// Returns a replacement hint for retired handoff verbs before clap parses them.
fn retired_handoff_verb(argv: &[String]) -> Option<String> {
    if argv.first().map(String::as_str) != Some("handoff") {
        return None;
    }
    match argv.get(1).map(String::as_str) {
        Some("new") => Some("renamed: use `pwf handoff add`.".to_string()),
        Some(verb @ ("done" | "cancel" | "reopen")) => Some(mirrored_pw_verb_hint(verb)),
        Some("refresh") => Some(
            "retired: the ledger is maintained automatically by handoff-mirroring verbs."
                .to_string(),
        ),
        _ => None,
    }
}

fn mirrored_pw_verb_hint(pw_verb: &str) -> String {
    format!(
        "retired: run `pwf {pw_verb} --id <pw-id>` \u{2014} a handoff-tagged task mirrors the \
         operation onto its handoff."
    )
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
