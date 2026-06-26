//! Internal Codex launcher shim for setting a compact thread name.
//!
//! Codex's interactive CLI has no `--name` flag. The supported naming surface is
//! the app-server `thread/name/set` method, so `pwf session --agent codex` runs
//! through this hidden shim: start a short-lived background worker that waits for
//! the just-created Codex thread, rename it, then replace the shim with `codex`.

use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};

pub(crate) const LAUNCH_COMMAND: &str = "__codex-thread-title";
pub(crate) const WORKER_COMMAND: &str = "__codex-thread-title-worker";

const PWF_FALLBACK_BINARY: &str = "pwf";
const CODEX_BINARY: &str = "codex";
const APP_SERVER: &str = "app-server";
const STDIO: &str = "--stdio";
const INITIALIZE: &str = "initialize";
const INITIALIZED: &str = "initialized";
const THREAD_LIST: &str = "thread/list";
const THREAD_NAME_SET: &str = "thread/name/set";
const TITLE_FLAG: &str = "--title";
const CWD_FLAG: &str = "--cwd";
const SINCE_FLAG: &str = "--since";
const ARG_SEPARATOR: &str = "--";
const CLIENT_NAME: &str = "pwf";
const CLIENT_TITLE: &str = "pwf codex thread title";
const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const MAX_RENAME_ATTEMPTS: usize = 40;
const RENAME_POLL_INTERVAL: Duration = Duration::from_millis(250);
const CREATED_AT_SLOP_SECONDS: u64 = 5;

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
    let invocation = match Invocation::parse(args) {
        Ok(invocation) => invocation,
        Err(_) => return 2,
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
    std::env::current_exe()
        .ok()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| PWF_FALLBACK_BINARY.to_string())
}

fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

struct Invocation {
    title: String,
    cwd: String,
    since: u64,
    codex_argv: Vec<String>,
}

impl Invocation {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut title = None;
        let mut cwd = None;
        let mut since = None;
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                ARG_SEPARATOR => {
                    let codex_argv = args[i + 1..].to_vec();
                    if codex_argv.is_empty() {
                        return Err("missing codex argv after --".to_string());
                    }
                    return Ok(Self {
                        title: title.ok_or_else(|| "missing --title".to_string())?,
                        cwd: cwd.ok_or_else(|| "missing --cwd".to_string())?,
                        since: since.ok_or_else(|| "missing --since".to_string())?,
                        codex_argv,
                    });
                }
                TITLE_FLAG => {
                    i += 1;
                    title = args.get(i).cloned();
                }
                CWD_FLAG => {
                    i += 1;
                    cwd = args.get(i).cloned();
                }
                SINCE_FLAG => {
                    i += 1;
                    since = Some(
                        args.get(i)
                            .ok_or_else(|| "missing --since value".to_string())?
                            .parse::<u64>()
                            .map_err(|_| "invalid --since value".to_string())?,
                    );
                }
                other => return Err(format!("unknown argument: {other}")),
            }
            i += 1;
        }
        Err("missing -- before codex argv".to_string())
    }

    fn prompt_prefix(&self) -> Option<&str> {
        self.codex_argv
            .last()
            .and_then(|prompt| prompt.lines().next())
            .filter(|line| !line.is_empty())
    }
}

struct AppServerClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl AppServerClient {
    fn start() -> Result<Self, String> {
        let mut child = Command::new(CODEX_BINARY)
            .args([APP_SERVER, STDIO])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("failed to start codex app-server: {e}"))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "failed to open app-server stdin".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "failed to open app-server stdout".to_string())?;
        let mut client = Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 0,
        };
        client.request(
            INITIALIZE,
            json!({
                "clientInfo": {
                    "name": CLIENT_NAME,
                    "title": CLIENT_TITLE,
                    "version": CLIENT_VERSION
                }
            }),
        )?;
        client.notify(INITIALIZED, json!({}))?;
        Ok(client)
    }

    fn find_thread(
        &mut self,
        cwd: &str,
        since: u64,
        prompt_prefix: Option<&str>,
    ) -> Result<Option<String>, String> {
        let result = self.request(
            THREAD_LIST,
            json!({
                "cwd": cwd,
                "limit": 10,
                "sortKey": "created_at",
                "sortDirection": "desc",
                "sourceKinds": ["cli"],
                "archived": false
            }),
        )?;
        let threshold = since.saturating_sub(CREATED_AT_SLOP_SECONDS);
        let thread_id = result
            .get("data")
            .and_then(Value::as_array)
            .and_then(|threads| {
                threads.iter().find_map(|thread| {
                    let created_at = thread.get("createdAt").and_then(Value::as_u64)?;
                    if created_at < threshold {
                        return None;
                    }
                    if let Some(prefix) = prompt_prefix {
                        let preview = thread.get("preview").and_then(Value::as_str)?;
                        if !preview.starts_with(prefix) {
                            return None;
                        }
                    }
                    thread.get("id").and_then(Value::as_str).map(str::to_string)
                })
            });
        Ok(thread_id)
    }

    fn set_thread_name(&mut self, thread_id: &str, title: &str) -> Result<(), String> {
        self.request(
            THREAD_NAME_SET,
            json!({
                "threadId": thread_id,
                "name": title
            }),
        )?;
        Ok(())
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let request = json!({
            "method": method,
            "id": id,
            "params": params
        });
        self.write_json(&request)?;
        self.read_response(id)
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        self.write_json(&json!({
            "method": method,
            "params": params
        }))
    }

    fn write_json(&mut self, value: &Value) -> Result<(), String> {
        serde_json::to_writer(&mut self.stdin, value).map_err(|e| e.to_string())?;
        self.stdin.write_all(b"\n").map_err(|e| e.to_string())?;
        self.stdin.flush().map_err(|e| e.to_string())
    }

    fn read_response(&mut self, id: u64) -> Result<Value, String> {
        loop {
            let mut line = String::new();
            let n = self
                .stdout
                .read_line(&mut line)
                .map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("app-server closed stdout".to_string());
            }
            let value: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
            if value.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = value.get("error") {
                return Err(error.to_string());
            }
            return Ok(value.get("result").cloned().unwrap_or(Value::Null));
        }
    }
}

impl Drop for AppServerClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
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

    #[test]
    fn invocation_parser_keeps_codex_prompt_as_one_arg() {
        let args = vec![
            TITLE_FLAG.to_string(),
            "title".to_string(),
            CWD_FLAG.to_string(),
            "/repo".to_string(),
            SINCE_FLAG.to_string(),
            "42".to_string(),
            ARG_SEPARATOR.to_string(),
            CODEX_BINARY.to_string(),
            ARG_SEPARATOR.to_string(),
            "prompt\n--danger".to_string(),
        ];
        let parsed = Invocation::parse(&args).unwrap();
        assert_eq!(parsed.title, "title");
        assert_eq!(parsed.cwd, "/repo");
        assert_eq!(parsed.since, 42);
        assert_eq!(parsed.codex_argv, vec!["codex", "--", "prompt\n--danger"]);
        assert_eq!(parsed.prompt_prefix(), Some("prompt"));
    }
}
