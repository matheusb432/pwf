//! Names Codex threads through a hidden launcher shim and the app-server API.
//!
//! A background worker names the new thread while the launcher replaces itself with Codex.
//! `invocation` parses the hidden argv protocol, and `app_server` owns JSON-RPC.

mod app_server;
mod invocation;

use std::{
    process::{Command, Stdio},
    time::Duration,
};

use app_server::AppServerClient;
use invocation::Invocation;

const MAX_RENAME_ATTEMPTS: usize = 40;
const RENAME_POLL_INTERVAL: Duration = Duration::from_millis(250);
/// Defines the argv tokens used by the hidden Codex launcher protocol.
#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct CodexThreadTitleProtocol {
    pub launch_command: &'static str,
    pub worker_command: &'static str,
    pub binary: &'static str,
    pub title_flag: &'static str,
    pub cwd_flag: &'static str,
    pub since_flag: &'static str,
    pub argument_separator: &'static str,
}

/// Runs the hidden Codex thread-title shim when `argv` requests it.
pub fn maybe_run(argv: &[String], protocol: &CodexThreadTitleProtocol) -> Option<i32> {
    match argv.first().map(String::as_str) {
        Some(command) if command == protocol.launch_command => {
            Some(run_launcher(&argv[1..], protocol))
        }
        Some(command) if command == protocol.worker_command => {
            Some(run_worker(&argv[1..], protocol))
        }
        _ => None,
    }
}

fn run_launcher(args: &[String], protocol: &CodexThreadTitleProtocol) -> i32 {
    let invocation = match Invocation::parse(args, protocol) {
        Ok(invocation) => invocation,
        Err(message) => {
            eprintln!("Error: {message}");
            return 2;
        }
    };
    spawn_worker(&invocation, protocol);
    run_codex(&invocation.codex_argv)
}

fn run_worker(args: &[String], protocol: &CodexThreadTitleProtocol) -> i32 {
    let Ok(invocation) = Invocation::parse(args, protocol) else {
        return 2;
    };
    match rename_when_thread_appears(
        &invocation.title,
        &invocation.cwd,
        invocation.since,
        invocation.prompt_prefix(),
        protocol.binary,
    ) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

fn spawn_worker(invocation: &Invocation, protocol: &CodexThreadTitleProtocol) {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let _ = Command::new(exe)
        .arg(protocol.worker_command)
        .arg(protocol.title_flag)
        .arg(&invocation.title)
        .arg(protocol.cwd_flag)
        .arg(&invocation.cwd)
        .arg(protocol.since_flag)
        .arg(invocation.since.to_string())
        .arg(protocol.argument_separator)
        .args(&invocation.codex_argv)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

fn run_codex(argv: &[String]) -> i32 {
    let Some((bin, rest)) = argv.split_first() else {
        eprintln!("Error: missing codex argv");
        return 2;
    };
    let mut cmd = Command::new(bin);
    cmd.args(rest);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        eprintln!("Error: {}", cmd.exec());
        1
    }
    #[cfg(not(unix))]
    {
        match cmd.status() {
            Ok(status) => status.code().unwrap_or(1),
            Err(e) => {
                eprintln!("Error: {e}");
                1
            }
        }
    }
}

fn rename_when_thread_appears(
    title: &str,
    cwd: &str,
    since: u64,
    prompt_prefix: Option<&str>,
    binary: &str,
) -> Result<(), String> {
    let mut client = AppServerClient::start(binary)?;
    for _ in 0..MAX_RENAME_ATTEMPTS {
        if let Some(thread_id) = client.find_thread(cwd, since, prompt_prefix)? {
            client.set_thread_name(&thread_id, title)?;
            return Ok(());
        }
        std::thread::sleep(RENAME_POLL_INTERVAL);
    }
    Err("timed out waiting for codex thread".to_string())
}
