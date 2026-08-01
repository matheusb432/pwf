//! Creates and names one Codex thread through the app-server protocol.

mod client;

use std::{fmt, marker::PhantomData};

use client::AppServerClient;
use serde_json::{Value, json};
use thiserror::Error;

use super::codex_reasoning_effort::CodexReasoningEffort;

#[derive(Debug, Clone, Copy)]
enum CodexAppServerOperation {
    ProcessStart,
    Initialize,
    ThreadStart,
    ThreadNameSet,
    ThreadDelete,
    Shutdown,
}

impl fmt::Display for CodexAppServerOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ProcessStart => "process start",
            Self::Initialize => "initialize",
            Self::ThreadStart => "thread/start",
            Self::ThreadNameSet => "thread/name/set",
            Self::ThreadDelete => "thread/delete",
            Self::Shutdown => "shutdown",
        })
    }
}

#[derive(Debug, Error)]
enum CodexAppServerError {
    #[error("{operation} failed: {message}", operation = CodexAppServerOperation::ProcessStart)]
    ProcessStart { message: String },
    #[error("{operation} failed: {message}")]
    Transport {
        operation: CodexAppServerOperation,
        message: String,
    },
    #[error("{operation} timed out after 10 seconds")]
    Timeout { operation: CodexAppServerOperation },
    #[error("{operation} returned a malformed response: {message}")]
    MalformedResponse {
        operation: CodexAppServerOperation,
        message: String,
    },
    #[error("{operation} failed: {message}")]
    Protocol {
        operation: CodexAppServerOperation,
        message: String,
    },
    #[error("{operation} failed: {message}", operation = CodexAppServerOperation::Shutdown)]
    Shutdown { message: String },
}

#[derive(Debug)]
struct CodexThreadId(String);

#[derive(Debug)]
struct Unnamed;

#[derive(Debug)]
struct Named;

#[derive(Debug)]
struct OwnedCodexThread<State> {
    id: CodexThreadId,
    state: PhantomData<State>,
}

#[derive(Debug)]
pub(super) struct NamedCodexThread(OwnedCodexThread<Named>);

impl NamedCodexThread {
    pub(super) fn into_id(self) -> String {
        self.0.id.0
    }
}

#[derive(Debug)]
enum PreparationFailure {
    Primary {
        primary: CodexAppServerError,
        shutdown: Option<CodexAppServerError>,
    },
    NamingAndCleanup {
        thread_id: String,
        naming: CodexAppServerError,
        cleanup: CodexAppServerError,
        shutdown: Option<CodexAppServerError>,
    },
    NamedShutdown {
        thread_id: String,
        shutdown: CodexAppServerError,
    },
}

/// Reports a failure before Codex dispatch begins.
#[derive(Debug)]
pub struct CodexThreadPreparationError {
    title: String,
    failure: Box<PreparationFailure>,
}

impl fmt::Display for CodexThreadPreparationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.failure.as_ref() {
            PreparationFailure::Primary { primary, shutdown } => {
                write!(
                    formatter,
                    "Codex app-server could not prepare thread '{}': {primary}",
                    self.title
                )?;
                write_shutdown_diagnostic(formatter, shutdown.as_ref())?;
                formatter.write_str(". Codex was not launched.")
            }
            PreparationFailure::NamingAndCleanup {
                thread_id,
                naming,
                cleanup,
                shutdown,
            } => {
                write!(
                    formatter,
                    "Codex app-server could not name thread '{}': {naming}; cleanup of unnamed thread '{thread_id}' also failed: {cleanup}",
                    self.title
                )?;
                write_shutdown_diagnostic(formatter, shutdown.as_ref())?;
                formatter.write_str(
                    ". Codex was not launched. Inspect the orphaned thread ID shown above.",
                )
            }
            PreparationFailure::NamedShutdown {
                thread_id,
                shutdown,
            } => write!(
                formatter,
                "Codex app-server named thread '{}' as '{}' but {shutdown}. Codex was not launched; the named thread was left intact.",
                thread_id, self.title
            ),
        }
    }
}

impl std::error::Error for CodexThreadPreparationError {}

fn write_shutdown_diagnostic(
    formatter: &mut fmt::Formatter<'_>,
    shutdown: Option<&CodexAppServerError>,
) -> fmt::Result {
    if let Some(shutdown) = shutdown {
        write!(formatter, "; app-server {shutdown}")?;
    }
    Ok(())
}

pub(super) fn start_and_name_thread(
    binary: &str,
    title: &str,
    repository: &str,
    model: Option<&str>,
    effort: CodexReasoningEffort,
) -> Result<NamedCodexThread, CodexThreadPreparationError> {
    let mut client =
        AppServerClient::start(binary).map_err(|failure| CodexThreadPreparationError {
            title: title.to_string(),
            failure: Box::new(PreparationFailure::Primary {
                primary: failure.primary,
                shutdown: failure.shutdown,
            }),
        })?;
    let preparation = prepare_thread(&mut client, title, repository, model, effort);
    let shutdown = client.shutdown();

    match preparation {
        Ok(named) => match shutdown {
            Ok(()) => Ok(NamedCodexThread(named)),
            Err(shutdown) => Err(CodexThreadPreparationError {
                title: title.to_string(),
                failure: Box::new(PreparationFailure::NamedShutdown {
                    thread_id: named.id.0,
                    shutdown,
                }),
            }),
        },
        Err(failure) => Err(CodexThreadPreparationError {
            title: title.to_string(),
            failure: Box::new(failure.with_shutdown(shutdown.err())),
        }),
    }
}

fn prepare_thread(
    client: &mut AppServerClient,
    title: &str,
    repository: &str,
    model: Option<&str>,
    effort: CodexReasoningEffort,
) -> Result<OwnedCodexThread<Named>, PreparationFailure> {
    client.initialize().map_err(PreparationFailure::primary)?;
    let unnamed =
        start_thread(client, repository, model, effort).map_err(PreparationFailure::primary)?;
    match name_thread(client, unnamed, title) {
        Ok(named) => Ok(named),
        Err((unnamed, naming)) => match delete_thread(client, unnamed) {
            Ok(()) => Err(PreparationFailure::primary(naming)),
            Err(cleanup_failure) => Err(PreparationFailure::NamingAndCleanup {
                thread_id: cleanup_failure.thread_id,
                naming,
                cleanup: cleanup_failure.error,
                shutdown: None,
            }),
        },
    }
}

impl PreparationFailure {
    fn primary(primary: CodexAppServerError) -> Self {
        Self::Primary {
            primary,
            shutdown: None,
        }
    }

    fn with_shutdown(self, shutdown: Option<CodexAppServerError>) -> Self {
        match self {
            Self::Primary { primary, .. } => Self::Primary { primary, shutdown },
            Self::NamingAndCleanup {
                thread_id,
                naming,
                cleanup,
                ..
            } => Self::NamingAndCleanup {
                thread_id,
                naming,
                cleanup,
                shutdown,
            },
            Self::NamedShutdown { .. } => self,
        }
    }
}

fn start_thread(
    client: &mut AppServerClient,
    repository: &str,
    model: Option<&str>,
    effort: CodexReasoningEffort,
) -> Result<OwnedCodexThread<Unnamed>, CodexAppServerError> {
    let result = client.request(
        CodexAppServerOperation::ThreadStart,
        "thread/start",
        &json!({
            "cwd": repository,
            "model": model,
            "config": {
                "model_reasoning_effort": effort.as_str(),
                "features": {
                    "hooks": false
                }
            }
        }),
    )?;
    OwnedCodexThread::from_thread_start_response(&result)
}

impl OwnedCodexThread<Unnamed> {
    fn from_thread_start_response(result: &Value) -> Result<Self, CodexAppServerError> {
        let id = result
            .get("thread")
            .and_then(|thread| thread.get("id"))
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| CodexAppServerError::MalformedResponse {
                operation: CodexAppServerOperation::ThreadStart,
                message: "result.thread.id is missing or empty".to_string(),
            })?;
        Ok(Self {
            id: CodexThreadId(id.to_string()),
            state: PhantomData,
        })
    }
}

fn name_thread(
    client: &mut AppServerClient,
    thread: OwnedCodexThread<Unnamed>,
    title: &str,
) -> Result<OwnedCodexThread<Named>, (OwnedCodexThread<Unnamed>, CodexAppServerError)> {
    let result = client.request(
        CodexAppServerOperation::ThreadNameSet,
        "thread/name/set",
        &json!({
            "threadId": &thread.id.0,
            "name": title
        }),
    );
    match result {
        Ok(_) => Ok(OwnedCodexThread {
            id: thread.id,
            state: PhantomData,
        }),
        Err(error) => Err((thread, error)),
    }
}

struct CleanupFailure {
    thread_id: String,
    error: CodexAppServerError,
}

fn delete_thread(
    client: &mut AppServerClient,
    thread: OwnedCodexThread<Unnamed>,
) -> Result<(), CleanupFailure> {
    let thread_id = thread.id.0;
    client
        .request(
            CodexAppServerOperation::ThreadDelete,
            "thread/delete",
            &json!({ "threadId": &thread_id }),
        )
        .map(|_| ())
        .map_err(|error| CleanupFailure { thread_id, error })
}

#[cfg(all(test, unix))]
pub(super) mod test_fixture {
    use std::{fs, os::unix::fs::symlink, path::PathBuf};

    use serde_json::Value;
    use tempfile::TempDir;

    pub(crate) const OWNED_THREAD_ID: &str = "thr-owned-by-this-request";

    pub(crate) struct AppServerFixture {
        _directory: TempDir,
        pub(crate) binary: PathBuf,
        pub(crate) log: PathBuf,
    }

    #[derive(Clone, Copy)]
    enum AppServerFixtureMode {
        Successful,
        NamingFailure { cleanup_fails: bool },
        OversizedResponseLine,
    }

    impl AppServerFixture {
        pub(crate) fn successful() -> Self {
            Self::new(AppServerFixtureMode::Successful)
        }

        pub(crate) fn naming_failure(cleanup_fails: bool) -> Self {
            Self::new(AppServerFixtureMode::NamingFailure { cleanup_fails })
        }

        pub(crate) fn oversized_response_line() -> Self {
            Self::new(AppServerFixtureMode::OversizedResponseLine)
        }

        fn new(mode: AppServerFixtureMode) -> Self {
            let directory = tempfile::tempdir().unwrap();
            let binary = directory.path().join("codex");
            let log = directory.path().join("requests.jsonl");
            symlink(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("src/session/codex_app_server/app_server_stub.sh"),
                &binary,
            )
            .unwrap();
            match mode {
                AppServerFixtureMode::Successful => {}
                AppServerFixtureMode::NamingFailure { cleanup_fails } => {
                    fs::write(directory.path().join("naming-fails"), "").unwrap();
                    if cleanup_fails {
                        fs::write(directory.path().join("cleanup-fails"), "").unwrap();
                    }
                }
                AppServerFixtureMode::OversizedResponseLine => {
                    fs::write(directory.path().join("oversized-response-line"), "").unwrap();
                }
            }
            Self {
                _directory: directory,
                binary,
                log,
            }
        }

        pub(crate) fn requests(&self) -> Vec<Value> {
            fs::read_to_string(&self.log)
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect()
        }
    }
}

#[cfg(all(test, unix))]
pub(super) use test_fixture::{AppServerFixture, OWNED_THREAD_ID};

#[cfg(all(test, unix))]
mod tests {
    use serde_json::{Value, json};

    use super::{start_and_name_thread, test_fixture::AppServerFixture};
    use crate::session::codex_reasoning_effort::CodexReasoningEffort;

    const OWNED_THREAD_ID: &str = "thr-owned-by-this-request";
    const TITLE: &str = "PWF-0153 - exact thread ownership";

    #[test]
    fn naming_failure_deletes_only_thread_id_returned_by_start() {
        let fixture = AppServerFixture::naming_failure(false);

        let error = start_and_name_thread(
            fixture.binary.to_str().unwrap(),
            TITLE,
            "/repo/pwf",
            Some("gpt-5.6"),
            CodexReasoningEffort::Max,
        )
        .unwrap_err();

        let requests = fixture.requests();
        assert_eq!(
            requests
                .iter()
                .filter_map(|request| request["method"].as_str())
                .collect::<Vec<_>>(),
            [
                "initialize",
                "initialized",
                "thread/start",
                "thread/name/set",
                "thread/delete",
            ]
        );
        assert_eq!(
            requests[2]["params"],
            json!({
                "cwd": "/repo/pwf",
                "model": "gpt-5.6",
                "config": {
                    "model_reasoning_effort": "max",
                    "features": {
                        "hooks": false
                    }
                }
            })
        );
        assert_eq!(requests[3]["params"]["threadId"], OWNED_THREAD_ID);
        assert_eq!(requests[4]["params"]["threadId"], OWNED_THREAD_ID);
        let transcript = requests
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!transcript.contains("thr-existing-before"));
        assert!(!transcript.contains("thr-created-concurrently"));
        assert!(!transcript.contains("turn/start"));

        let diagnostic = error.to_string();
        assert!(diagnostic.contains("thread/name/set"), "{diagnostic}");
        assert!(diagnostic.contains(TITLE), "{diagnostic}");
        assert!(
            diagnostic.contains("Codex was not launched"),
            "{diagnostic}"
        );
    }

    #[test]
    fn cleanup_failure_retains_both_errors_and_exact_orphan_id() {
        let fixture = AppServerFixture::naming_failure(true);

        let error = start_and_name_thread(
            fixture.binary.to_str().unwrap(),
            TITLE,
            "/repo/pwf",
            None,
            CodexReasoningEffort::High,
        )
        .unwrap_err();

        let requests = fixture.requests();
        assert_eq!(requests[2]["params"]["model"], Value::Null);
        let diagnostic = error.to_string();
        assert!(
            diagnostic.contains("name denied by fixture"),
            "{diagnostic}"
        );
        assert!(
            diagnostic.contains("delete denied by fixture"),
            "{diagnostic}"
        );
        assert!(diagnostic.contains(OWNED_THREAD_ID), "{diagnostic}");
        assert!(diagnostic.contains(TITLE), "{diagnostic}");
        assert!(
            diagnostic.contains("Codex was not launched"),
            "{diagnostic}"
        );
    }

    #[test]
    fn oversized_response_line_is_rejected_at_the_reader_boundary() {
        let fixture = AppServerFixture::oversized_response_line();

        let error = start_and_name_thread(
            fixture.binary.to_str().unwrap(),
            TITLE,
            "/repo/pwf",
            None,
            CodexReasoningEffort::High,
        )
        .unwrap_err();

        let diagnostic = error.to_string();
        assert!(
            diagnostic.contains(
                "initialize returned a malformed response: response line exceeds 1048576-byte limit"
            ),
            "{diagnostic}"
        );
        assert!(
            diagnostic.contains("Codex was not launched"),
            "{diagnostic}"
        );
        let requests = fixture.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["method"], "initialize");
        assert!(!requests[0].to_string().contains("turn/start"));
    }
}
