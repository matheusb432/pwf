//! Internal Codex launcher shim for setting a compact thread name.
//!
//! Codex's interactive CLI has no `--name` flag. The supported naming surface is
//! the app-server `thread/name/set` method, so `pwf session --agent codex` runs
//! through this hidden shim: start a short-lived background worker that waits for
//! the just-created Codex thread, rename it, then replace the shim with `codex`.
//!
//! This root owns the launcher/worker orchestration; the argv protocol parser
//! lives in [`invocation`] and the app-server JSON-RPC client in [`app_server`].

mod app_server;
mod invocation;

use std::{
    process::{Command, Stdio},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use app_server::AppServerClient;
use invocation::Invocation;

pub(crate) const LAUNCH_COMMAND: &str = "__codex-thread-title";
pub(crate) const WORKER_COMMAND: &str = "__codex-thread-title-worker";

const PWF_FALLBACK_BINARY: &str = "pwf";
const CODEX_BINARY: &str = "codex";
const TITLE_FLAG: &str = "--title";
const CWD_FLAG: &str = "--cwd";
const SINCE_FLAG: &str = "--since";
const ARG_SEPARATOR: &str = "--";
const MAX_RENAME_ATTEMPTS: usize = 40;
const RENAME_POLL_INTERVAL: Duration = Duration::from_millis(250);

pub(crate) fn launch_argv(title: String, cwd: String, prompt: String) -> Vec<String> {
    let mut argv = vec![
        current_pwf_exe(),
        LAUNCH_COMMAND.to_string(),
        TITLE_FLAG.to_string(),
        title,
        CWD_FLAG.to_string(),
        cwd,
        SINCE_FLAG.to_string(),
        now_unix_seconds().to_string(),
        ARG_SEPARATOR.to_string(),
        CODEX_BINARY.to_string(),
        ARG_SEPARATOR.to_string(),
    ];
    argv.push(prompt);
    argv
}

/// Runs the hidden Codex thread-title shim when `argv` requests it.
pub fn maybe_run(argv: &[String]) -> Option<i32> {
    match argv.first().map(String::as_str) {
        Some(LAUNCH_COMMAND) => Some(run_launcher(&argv[1..])),
        Some(WORKER_COMMAND) => Some(run_worker(&argv[1..])),
        _ => None,
    }
}

fn run_launcher(args: &[String]) -> i32 {
    let invocation = match Invocation::parse(args) {
        Ok(invocation) => invocation,
        Err(message) => {
            eprintln!("Error: {message}");
            return 2;
        }
    };
    spawn_worker(&invocation);
    run_codex(&invocation.codex_argv)
}

fn run_worker(args: &[String]) -> i32 {
    let Ok(invocation) = Invocation::parse(args) else {
        return 2;
    };
    match rename_when_thread_appears(
        &invocation.title,
        &invocation.cwd,
        invocation.since,
        invocation.prompt_prefix(),
    ) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

fn spawn_worker(invocation: &Invocation) {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let _ = Command::new(exe)
        .arg(WORKER_COMMAND)
        .arg(TITLE_FLAG)
        .arg(&invocation.title)
        .arg(CWD_FLAG)
        .arg(&invocation.cwd)
        .arg(SINCE_FLAG)
        .arg(invocation.since.to_string())
        .arg(ARG_SEPARATOR)
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
) -> Result<(), String> {
    let mut client = AppServerClient::start()?;
    for _ in 0..MAX_RENAME_ATTEMPTS {
        if let Some(thread_id) = client.find_thread(cwd, since, prompt_prefix)? {
            client.set_thread_name(&thread_id, title)?;
            return Ok(());
        }
        std::thread::sleep(RENAME_POLL_INTERVAL);
    }
    Err("timed out waiting for codex thread".to_string())
}

fn current_pwf_exe() -> String {
    std::env::current_exe().ok().map_or_else(
        || PWF_FALLBACK_BINARY.to_string(),
        |path| path.to_string_lossy().into_owned(),
    )
}

fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_argv_wraps_codex_with_title_and_guarded_prompt() {
        let argv = launch_argv(
            "PWF-0001 - do the thing".to_string(),
            "/repo".to_string(),
            "prompt\n--danger".to_string(),
        );
        assert_eq!(argv[1], LAUNCH_COMMAND);
        assert!(argv.contains(&"PWF-0001 - do the thing".to_string()));
        assert!(argv.contains(&"/repo".to_string()));
        let codex_pos = argv.iter().position(|arg| arg == CODEX_BINARY).unwrap();
        assert_eq!(argv[codex_pos + 1], ARG_SEPARATOR);
        assert_eq!(argv.last().unwrap(), "prompt\n--danger");
    }
}
