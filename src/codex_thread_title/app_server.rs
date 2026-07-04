//! JSON-RPC client for Codex's `app-server --stdio`: initialize, list threads,
//! and set a thread name over line-delimited JSON.

use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStdin, ChildStdout, Command, Stdio},
};

use serde_json::{Value, json};

use super::CODEX_BINARY;

const APP_SERVER: &str = "app-server";
const STDIO: &str = "--stdio";
const INITIALIZE: &str = "initialize";
const INITIALIZED: &str = "initialized";
const THREAD_LIST: &str = "thread/list";
const THREAD_NAME_SET: &str = "thread/name/set";
const CLIENT_NAME: &str = "pwf";
const CLIENT_TITLE: &str = "pwf codex thread title";
const CLIENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const CREATED_AT_SLOP_SECONDS: u64 = 5;

pub(super) struct AppServerClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl AppServerClient {
    pub(super) fn start() -> Result<Self, String> {
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
            &json!({
                "clientInfo": {
                    "name": CLIENT_NAME,
                    "title": CLIENT_TITLE,
                    "version": CLIENT_VERSION
                }
            }),
        )?;
        client.notify(INITIALIZED, &json!({}))?;
        Ok(client)
    }

    pub(super) fn find_thread(
        &mut self,
        cwd: &str,
        since: u64,
        prompt_prefix: Option<&str>,
    ) -> Result<Option<String>, String> {
        let result = self.request(
            THREAD_LIST,
            &json!({
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
            .and_then(|threads| matching_thread_id(threads, threshold, prompt_prefix));
        Ok(thread_id)
    }

    pub(super) fn set_thread_name(&mut self, thread_id: &str, title: &str) -> Result<(), String> {
        self.request(
            THREAD_NAME_SET,
            &json!({
                "threadId": thread_id,
                "name": title
            }),
        )?;
        Ok(())
    }

    fn request(&mut self, method: &str, params: &Value) -> Result<Value, String> {
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

    fn notify(&mut self, method: &str, params: &Value) -> Result<(), String> {
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

/// Pure decision over a `thread/list` response: the first (newest-first) thread
/// created at/after `threshold` whose `preview` starts with `prompt_prefix`
/// (when given). A thread missing `createdAt` — or missing `preview` while a
/// prefix filter is set — never matches.
fn matching_thread_id(
    threads: &[Value],
    threshold: u64,
    prompt_prefix: Option<&str>,
) -> Option<String> {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thread(id: &str, created_at: u64, preview: &str) -> Value {
        json!({ "id": id, "createdAt": created_at, "preview": preview })
    }

    #[test]
    fn newest_thread_past_threshold_matches() {
        let threads = vec![
            thread("t-new", 100, "prompt line"),
            thread("t-old", 10, "x"),
        ];
        assert_eq!(
            matching_thread_id(&threads, 50, None),
            Some("t-new".to_string())
        );
    }

    #[test]
    fn threads_created_before_threshold_never_match() {
        let threads = vec![thread("t-old", 10, "prompt line")];
        assert_eq!(matching_thread_id(&threads, 50, None), None);
    }

    #[test]
    fn prompt_prefix_filters_previews() {
        let threads = vec![
            thread("t-other", 100, "someone else's session"),
            thread("t-mine", 100, "prompt line two"),
        ];
        assert_eq!(
            matching_thread_id(&threads, 50, Some("prompt line")),
            Some("t-mine".to_string())
        );
    }

    #[test]
    fn missing_created_at_or_preview_is_skipped_not_matched() {
        let threads = vec![
            json!({ "id": "t-no-created", "preview": "prompt line" }),
            json!({ "id": "t-no-preview", "createdAt": 100 }),
            thread("t-ok", 100, "prompt line"),
        ];
        assert_eq!(
            matching_thread_id(&threads, 50, Some("prompt line")),
            Some("t-ok".to_string())
        );
    }
}
