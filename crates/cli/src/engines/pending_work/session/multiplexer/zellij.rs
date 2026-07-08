//! zellij backend for the [`MultiplexerDriver`] seam. Owns everything
//! zellij-specific: the `zellij` binary calls, its CLI argument shapes, and its
//! exit-status quirks. The command vectors are built by pure functions so the
//! contract is unit-tested without spawning zellij.

use std::process::Command;

use super::{MultiplexerDriver, NewTabError};

/// `zellij --session <s> action new-tab --cwd <cwd> --name <tab> -- <argv…>`.
fn new_tab_argv(session: &str, cwd: &str, tab: &str, argv: &[String]) -> Vec<String> {
    let mut v = vec![
        "--session".to_string(),
        session.to_string(),
        "action".to_string(),
        "new-tab".to_string(),
        "--cwd".to_string(),
        cwd.to_string(),
        "--name".to_string(),
        tab.to_string(),
        "--".to_string(),
    ];
    v.extend(argv.iter().cloned());
    v
}

/// Classify a `new-tab` invocation's result.
///
/// zellij exits **0 even when the target session is missing or EXITED**, writing
/// `Session '<s>' not found` to stderr — so a successful exit status is necessary
/// but not sufficient. The stderr message is the authoritative failure signal; a
/// bare status check reports a phantom success and skips the resurrect-and-retry
/// fallback (FR-0009).
fn classify_new_tab(success: bool, stderr: &str) -> Result<(), NewTabError> {
    if stderr.contains("not found") {
        return Err(NewTabError::SessionNotFound);
    }
    if success {
        return Ok(());
    }
    Err(NewTabError::Other(stderr.trim().to_string()))
}

/// `zellij attach --create-background <session>`.
fn ensure_session_argv(session: &str) -> Vec<String> {
    vec![
        "attach".to_string(),
        "--create-background".to_string(),
        session.to_string(),
    ]
}

/// Real backend: shells out to the `zellij` binary.
pub(in crate::engines::pending_work) struct RealZellij;

impl MultiplexerDriver for RealZellij {
    fn available(&self) -> bool {
        Command::new("zellij")
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    }

    fn new_tab(
        &self,
        session: &str,
        cwd: &str,
        tab: &str,
        argv: &[String],
    ) -> Result<(), NewTabError> {
        let output = Command::new("zellij")
            .args(new_tab_argv(session, cwd, tab, argv))
            .output()
            .map_err(|e| NewTabError::Other(e.to_string()))?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        classify_new_tab(output.status.success(), &stderr)
    }

    fn ensure_session(&self, session: &str) -> Result<(), String> {
        let output = Command::new("zellij")
            .args(ensure_session_argv(session))
            .output()
            .map_err(|e| e.to_string())?;
        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use super::*;

    #[test]
    fn new_tab_argv_passes_command_through_after_double_dash() {
        let got = new_tab_argv(
            "cfg",
            "/repo",
            "CFG-0009",
            &[
                "claude".into(),
                "--name".into(),
                "t".into(),
                "multi\nline prompt".into(),
            ],
        );
        assert_eq!(
            got,
            vec![
                "--session",
                "cfg",
                "action",
                "new-tab",
                "--cwd",
                "/repo",
                "--name",
                "CFG-0009",
                "--",
                "claude",
                "--name",
                "t",
                "multi\nline prompt",
            ]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>()
        );
        // the multi-line prompt is a single argv element — escaping-free contract.
        assert_eq!(got.last().unwrap(), "multi\nline prompt");
    }

    #[test]
    fn classify_treats_live_success_as_ok() {
        assert_matches!(classify_new_tab(true, ""), Ok(()));
    }

    #[test]
    fn classify_treats_exit_zero_not_found_as_session_not_found() {
        // Regression (PWF-0075): against a missing/EXITED session zellij exits 0
        // and writes "not found" to stderr. Trusting the status reported a phantom
        // dispatch into a dead session; the message must win.
        let stderr = "Session 'ssh' not found. The following sessions are active:";
        assert_matches!(
            classify_new_tab(true, stderr),
            Err(NewTabError::SessionNotFound)
        );
    }

    #[test]
    fn classify_maps_other_failures_to_trimmed_other() {
        assert_matches!(
            classify_new_tab(false, "  boom  "),
            Err(NewTabError::Other(m)) if m == "boom"
        );
    }

    #[test]
    fn ensure_session_argv_is_create_background() {
        assert_eq!(
            ensure_session_argv("cfg"),
            vec!["attach", "--create-background", "cfg"]
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>()
        );
    }
}
