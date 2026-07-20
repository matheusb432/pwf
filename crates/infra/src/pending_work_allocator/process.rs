use std::{
    path::{Path, PathBuf},
    process::{ExitStatus, Output},
};

use pwf_application::handoff::ports::{AllocatePendingWork, PendingWorkAllocatorClient};
use pwf_domain::pending_work::WorkItemId;

/// Invokes the legacy pending-work allocator executable when configured.
#[derive(Debug, Clone, Default)]
pub struct ProcessPendingWorkAllocator {
    script: Option<PathBuf>,
}

impl ProcessPendingWorkAllocator {
    /// Configures the optional executable used by external allocation.
    pub fn new(script: Option<PathBuf>) -> Self {
        Self { script }
    }
}

/// Reports invalid successful allocator stdout.
#[derive(Debug, thiserror::Error)]
pub enum AllocationOutputError {
    /// Successful stdout was not UTF-8.
    #[error("allocator stdout is not valid UTF-8: {source}")]
    InvalidUtf8 {
        /// UTF-8 validation failure retaining the original bytes.
        #[source]
        source: std::string::FromUtf8Error,
    },
    /// The first line did not match the allocator protocol.
    #[error("pw-add output parse error (stdout: {stdout})")]
    Protocol {
        /// Complete successful stdout.
        stdout: String,
    },
    /// The bracketed value was not a valid pending-work identifier.
    #[error("pw-add output parse error (stdout: {stdout}; invalid id: {value})")]
    InvalidIdentifier {
        /// Invalid bracketed identifier.
        value: String,
        /// Complete successful stdout.
        stdout: String,
    },
}

/// Reports process launch, status, or output failures.
#[derive(Debug, thiserror::Error)]
pub enum ProcessPendingWorkAllocatorError {
    /// External allocation was requested without a configured executable.
    #[error("external pending-work allocator is not configured")]
    Unconfigured,
    /// The configured executable could not be launched.
    #[error("cannot launch pending-work allocator {}: {source}", script.display())]
    Spawn {
        /// Configured executable path.
        script: PathBuf,
        /// Process spawn failure.
        #[source]
        source: std::io::Error,
    },
    /// The allocator exited unsuccessfully.
    #[error("pending-work allocator exited with {status}; stderr: {stderr}")]
    Unsuccessful {
        /// Nonzero process status.
        status: ExitStatus,
        /// Lossy stdout retained for diagnosis.
        stdout: String,
        /// Lossy stderr retained for diagnosis.
        stderr: String,
    },
    /// A successful process returned invalid stdout.
    #[error(
        "pending-work allocator returned invalid output with {status}; stderr: {stderr}: {source}"
    )]
    Output {
        /// Successful process status.
        status: ExitStatus,
        /// Lossy stderr retained for diagnosis.
        stderr: String,
        /// Strict stdout parsing failure.
        #[source]
        source: AllocationOutputError,
    },
}

impl PendingWorkAllocatorClient for ProcessPendingWorkAllocator {
    type Error = ProcessPendingWorkAllocatorError;

    fn allocate(&self, request: &AllocatePendingWork) -> Result<WorkItemId, Self::Error> {
        let script = self
            .script
            .as_ref()
            .ok_or(ProcessPendingWorkAllocatorError::Unconfigured)?;
        let output = run_allocator(script, request).map_err(|source| {
            ProcessPendingWorkAllocatorError::Spawn {
                script: script.clone(),
                source,
            }
        })?;
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        if !output.status.success() {
            return Err(ProcessPendingWorkAllocatorError::Unsuccessful {
                status: output.status,
                stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                stderr,
            });
        }
        parse_allocation_output(&output.stdout).map_err(|source| {
            ProcessPendingWorkAllocatorError::Output {
                status: output.status,
                stderr,
                source,
            }
        })
    }
}

#[rustfmt::skip]
fn run_allocator(script: &Path, request: &AllocatePendingWork) -> Result<Output, std::io::Error> {
    // FIXME: Pass the exact handoff path through the external allocator protocol; newest-file discovery can attach concurrent additions to the wrong handoff.
    std::process::Command::new(script)
        .arg("add")
        .arg("--config-path")
        .arg(&request.config_path)
        .args([
            "--date",
            request.created.as_str(),
            request.project.as_ref(),
            "--tag",
            pwf_domain::pending_work::HANDOFF_TAG,
            "--continue-handoff",
        ])
        .output()
}

fn parse_allocation_output(output: &[u8]) -> Result<WorkItemId, AllocationOutputError> {
    let stdout = String::from_utf8(output.to_vec())
        .map_err(|source| AllocationOutputError::InvalidUtf8 { source })?;
    let first_line = stdout
        .lines()
        .next()
        .ok_or_else(|| AllocationOutputError::Protocol {
            stdout: stdout.clone(),
        })?;
    let remainder = first_line.strip_prefix("ADDED PWF TASK [").ok_or_else(|| {
        AllocationOutputError::Protocol {
            stdout: stdout.clone(),
        }
    })?;
    let (value, suffix) =
        remainder
            .split_once(']')
            .ok_or_else(|| AllocationOutputError::Protocol {
                stdout: stdout.clone(),
            })?;
    if !suffix.is_empty() && !suffix.starts_with(' ') {
        return Err(AllocationOutputError::Protocol { stdout });
    }
    let value = value.to_string();
    WorkItemId::try_new(&value)
        .map_err(|_| AllocationOutputError::InvalidIdentifier { value, stdout })
}

#[cfg(test)]
mod tests {
    use std::{assert_matches, path::PathBuf};

    use pwf_application::handoff::ports::{AllocatePendingWork, PendingWorkAllocatorClient};
    use pwf_domain::pending_work::{ProjectName, Timestamp};

    use super::{
        AllocationOutputError, ProcessPendingWorkAllocator, ProcessPendingWorkAllocatorError,
        parse_allocation_output,
    };

    #[cfg(unix)]
    fn allocator_fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join(name)
    }

    #[test]
    fn parser_accepts_only_the_exact_first_line_protocol() {
        let parsed = parse_allocation_output(
            b"ADDED PWF TASK [PWF-0001] pwf :: title\nignored second line\n",
        )
        .unwrap();
        assert_eq!(parsed.as_ref(), "PWF-0001");

        for output in [
            &b"noise [PWF-0001]\n"[..],
            &b"prefix ADDED PWF TASK [PWF-0001]\n"[..],
            &b"ADDED PWF TASK [not-an-id]\n"[..],
            &b"\nADDED PWF TASK [PWF-0001]\n"[..],
        ] {
            assert!(parse_allocation_output(output).is_err(), "{output:?}");
        }
    }

    #[test]
    fn parser_rejects_invalid_utf8_without_lossy_replacement() {
        let error = parse_allocation_output(b"ADDED PWF TASK [PWF-0001]\n\xff").unwrap_err();

        assert_matches!(error, AllocationOutputError::InvalidUtf8 { .. });
    }

    #[cfg(unix)]
    #[test]
    fn process_client_passes_the_complete_canonical_argument_vector() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let argument_log = temporary_directory.path().join("arguments.txt");
        let client = ProcessPendingWorkAllocator::new(Some(allocator_fixture(
            "pending-work-allocator-success.sh",
        )));

        let parsed = client
            .allocate(&AllocatePendingWork {
                config_path: argument_log.clone(),
                created: Timestamp::new("2026-01-01"),
                project: ProjectName::try_new("test-project").unwrap(),
            })
            .unwrap();

        assert_eq!(parsed.as_ref(), "PWF-0001");
        assert_eq!(
            std::fs::read_to_string(argument_log).unwrap(),
            format!(
                "add\n--config-path\n{}\n--date\n2026-01-01\ntest-project\n--tag\nhandoff\n--continue-handoff\n",
                temporary_directory.path().join("arguments.txt").display()
            )
        );
    }

    #[cfg(unix)]
    #[test]
    fn process_client_preserves_non_utf8_config_path_bytes_in_the_complete_argument_vector() {
        use std::{
            ffi::OsString,
            os::unix::ffi::{OsStrExt as _, OsStringExt as _},
        };

        let temporary_directory = tempfile::tempdir().unwrap();
        let client = ProcessPendingWorkAllocator::new(Some(allocator_fixture(
            "pending-work-allocator-success.sh",
        )));
        let config_path = temporary_directory
            .path()
            .join(OsString::from_vec(b"pending-\xff-work.json".to_vec()));

        let parsed = client
            .allocate(&AllocatePendingWork {
                config_path: config_path.clone(),
                created: Timestamp::new("2026-01-01"),
                project: ProjectName::try_new("test-project").unwrap(),
            })
            .unwrap();

        assert_eq!(parsed.as_ref(), "PWF-0001");
        let mut expected = b"add\n--config-path\n".to_vec();
        expected.extend_from_slice(config_path.as_os_str().as_bytes());
        expected.extend_from_slice(
            b"\n--date\n2026-01-01\ntest-project\n--tag\nhandoff\n--continue-handoff\n",
        );
        assert_eq!(std::fs::read(config_path).unwrap(), expected);
    }

    #[cfg(unix)]
    #[test]
    fn nonzero_exit_retains_status_and_stderr_even_with_valid_stdout() {
        let client = ProcessPendingWorkAllocator::new(Some(allocator_fixture(
            "pending-work-allocator-nonzero.sh",
        )));

        let error = client
            .allocate(&AllocatePendingWork {
                config_path: PathBuf::from("/tmp/pending-work.json"),
                created: Timestamp::new("2026-01-01"),
                project: ProjectName::try_new("test-project").unwrap(),
            })
            .unwrap_err();

        assert_matches!(
            error,
            ProcessPendingWorkAllocatorError::Unsuccessful {
                ref status,
                ref stdout,
                ref stderr,
                ..
            } if status.code() == Some(23)
                && stdout == "ADDED PWF TASK [PWF-0001] test-project :: title\n"
                && stderr == "allocation failed\n"
        );
    }

    #[test]
    fn unconfigured_client_fails_only_when_allocation_is_requested() {
        let client = ProcessPendingWorkAllocator::new(None);

        let error = client
            .allocate(&AllocatePendingWork {
                config_path: PathBuf::from("/tmp/pending-work.json"),
                created: Timestamp::new("2026-01-01"),
                project: ProjectName::try_new("test-project").unwrap(),
            })
            .unwrap_err();

        assert_matches!(error, ProcessPendingWorkAllocatorError::Unconfigured);
    }
}
