mod reader;

use std::{
    io::Write as _,
    process::{Child, ChildStdin, Stdio},
    sync::mpsc::{Receiver, RecvTimeoutError},
    thread,
    time::{Duration, Instant},
};

use reader::{ReaderFailure, start_response_reader};
use serde_json::{Value, json};

use super::{CodexAppServerError, CodexAppServerOperation};
use crate::session::ProcessEnvironment;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
const SHUTDOWN_POLL_INTERVAL: Duration = Duration::from_millis(10);

pub(super) struct AppServerStartFailure {
    pub(super) primary: CodexAppServerError,
    pub(super) shutdown: Option<CodexAppServerError>,
}

pub(super) struct AppServerClient {
    child: Child,
    stdin: Option<ChildStdin>,
    responses: Receiver<Result<Value, ReaderFailure>>,
    request_id_next: u64,
}

impl AppServerClient {
    pub(super) fn start(
        binary: &str,
        environment: &ProcessEnvironment,
    ) -> Result<Self, AppServerStartFailure> {
        let mut command = environment.command(binary);
        command
            .args(["app-server", "--stdio"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = command.spawn().map_err(|error| AppServerStartFailure {
            primary: CodexAppServerError::ProcessStart {
                message: error.to_string(),
            },
            shutdown: None,
        })?;
        let Some(stdin) = child.stdin.take() else {
            return Err(fail_started_child(
                child,
                CodexAppServerError::ProcessStart {
                    message: "app-server stdin pipe is unavailable".to_string(),
                },
            ));
        };
        let Some(stdout) = child.stdout.take() else {
            drop(stdin);
            return Err(fail_started_child(
                child,
                CodexAppServerError::ProcessStart {
                    message: "app-server stdout pipe is unavailable".to_string(),
                },
            ));
        };
        let responses = match start_response_reader(stdout) {
            Ok(responses) => responses,
            Err(error) => {
                drop(stdin);
                return Err(fail_started_child(
                    child,
                    CodexAppServerError::ProcessStart {
                        message: format!("response reader could not start: {error}"),
                    },
                ));
            }
        };

        Ok(Self {
            child,
            stdin: Some(stdin),
            responses,
            request_id_next: 0,
        })
    }

    pub(super) fn initialize(&mut self) -> Result<(), CodexAppServerError> {
        self.request(
            CodexAppServerOperation::Initialize,
            "initialize",
            &json!({
                "clientInfo": {
                    "name": "pwf",
                    "title": "pwf Codex session preparation",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": {
                    "experimentalApi": true
                }
            }),
        )?;
        self.notify(
            CodexAppServerOperation::Initialize,
            "initialized",
            &json!({}),
        )
    }

    pub(super) fn request(
        &mut self,
        operation: CodexAppServerOperation,
        method: &str,
        params: &Value,
    ) -> Result<Value, CodexAppServerError> {
        let request_id = self.request_id_next;
        self.request_id_next += 1;
        self.write_json(
            operation,
            &json!({
                "method": method,
                "id": request_id,
                "params": params
            }),
        )?;
        wait_for_response(&self.responses, operation, request_id)
    }

    fn notify(
        &mut self,
        operation: CodexAppServerOperation,
        method: &str,
        params: &Value,
    ) -> Result<(), CodexAppServerError> {
        self.write_json(
            operation,
            &json!({
                "method": method,
                "params": params
            }),
        )
    }

    fn write_json(
        &mut self,
        operation: CodexAppServerOperation,
        value: &Value,
    ) -> Result<(), CodexAppServerError> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| CodexAppServerError::Transport {
                operation,
                message: "app-server stdin is closed".to_string(),
            })?;
        serde_json::to_writer(&mut *stdin, value).map_err(|error| {
            CodexAppServerError::Transport {
                operation,
                message: error.to_string(),
            }
        })?;
        stdin
            .write_all(b"\n")
            .and_then(|()| stdin.flush())
            .map_err(|error| CodexAppServerError::Transport {
                operation,
                message: error.to_string(),
            })
    }

    pub(super) fn shutdown(self) -> Result<(), CodexAppServerError> {
        let Self {
            mut child,
            stdin,
            responses,
            request_id_next: _,
        } = self;
        drop(stdin);
        drop(responses);
        terminate_and_reap(&mut child)
    }
}

fn wait_for_response(
    responses: &Receiver<Result<Value, ReaderFailure>>,
    operation: CodexAppServerOperation,
    request_id: u64,
) -> Result<Value, CodexAppServerError> {
    let deadline = Instant::now() + REQUEST_TIMEOUT;
    loop {
        let timeout = response_timeout_remaining(operation, deadline)?;
        let received = responses.recv_timeout(timeout);
        if let Some(outcome) = decode_received_response(received, operation, request_id) {
            return outcome;
        }
    }
}

fn response_timeout_remaining(
    operation: CodexAppServerOperation,
    deadline: Instant,
) -> Result<Duration, CodexAppServerError> {
    let now = Instant::now();
    if now >= deadline {
        return Err(CodexAppServerError::Timeout { operation });
    }
    Ok(deadline - now)
}

fn decode_received_response(
    received: Result<Result<Value, ReaderFailure>, RecvTimeoutError>,
    operation: CodexAppServerOperation,
    request_id: u64,
) -> Option<Result<Value, CodexAppServerError>> {
    match received {
        Ok(Ok(response)) => matching_response(&response, operation, request_id),
        Ok(Err(ReaderFailure::Transport(message))) => {
            Some(Err(CodexAppServerError::Transport { operation, message }))
        }
        Ok(Err(ReaderFailure::Malformed(message))) => {
            Some(Err(CodexAppServerError::MalformedResponse {
                operation,
                message,
            }))
        }
        Ok(Err(ReaderFailure::ResponseLineBytesLimitExceeded { bytes_max })) => {
            Some(Err(CodexAppServerError::MalformedResponse {
                operation,
                message: format!("response line exceeds {bytes_max}-byte limit"),
            }))
        }
        Ok(Err(ReaderFailure::Closed)) | Err(RecvTimeoutError::Disconnected) => {
            Some(Err(CodexAppServerError::Transport {
                operation,
                message: "app-server closed stdout".to_string(),
            }))
        }
        Err(RecvTimeoutError::Timeout) => Some(Err(CodexAppServerError::Timeout { operation })),
    }
}

fn matching_response(
    response: &Value,
    operation: CodexAppServerOperation,
    request_id: u64,
) -> Option<Result<Value, CodexAppServerError>> {
    if response.get("id").and_then(Value::as_u64) != Some(request_id) {
        return None;
    }
    if let Some(error) = response.get("error") {
        return Some(Err(protocol_error(operation, error)));
    }
    Some(
        response
            .get("result")
            .cloned()
            .ok_or_else(|| CodexAppServerError::MalformedResponse {
                operation,
                message: "matching response has no result".to_string(),
            }),
    )
}

fn fail_started_child(mut child: Child, primary: CodexAppServerError) -> AppServerStartFailure {
    drop(child.stdin.take());
    drop(child.stdout.take());
    AppServerStartFailure {
        primary,
        shutdown: terminate_and_reap(&mut child).err(),
    }
}

fn terminate_and_reap(child: &mut Child) -> Result<(), CodexAppServerError> {
    let termination_error = child.kill().err().map(|error| error.to_string());
    let deadline = Instant::now() + SHUTDOWN_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return Ok(()),
            Ok(None) if Instant::now() < deadline => {
                thread::sleep(SHUTDOWN_POLL_INTERVAL);
            }
            Ok(None) => {
                return Err(CodexAppServerError::Shutdown {
                    message: shutdown_timeout_message(termination_error),
                });
            }
            Err(error) => {
                return Err(CodexAppServerError::Shutdown {
                    message: error.to_string(),
                });
            }
        }
    }
}

fn shutdown_timeout_message(termination_error: Option<String>) -> String {
    termination_error.map_or_else(
        || "process did not exit within 2 seconds".to_string(),
        |error| {
            format!("process termination failed ({error}) and it did not exit within 2 seconds")
        },
    )
}

fn protocol_error(operation: CodexAppServerOperation, error: &Value) -> CodexAppServerError {
    let code = error
        .get("code")
        .and_then(Value::as_i64)
        .map_or_else(|| "unknown".to_string(), |code| code.to_string());
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .map_or_else(|| error.to_string(), ToString::to_string);
    CodexAppServerError::Protocol {
        operation,
        message: format!("server returned code {code}: {message}"),
    }
}
