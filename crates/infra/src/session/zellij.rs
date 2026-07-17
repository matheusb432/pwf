//! Owns the Zellij command protocol and provider-specific failure classification.

use std::process::Command;

use pwf_application::pending_work::session::TabOpenError;

pub(super) fn available() -> bool {
    Command::new("zellij")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

pub(super) fn open_tab(
    session: &str,
    cwd: &str,
    tab: &str,
    argv: &[String],
) -> Result<(), TabOpenError> {
    let output = Command::new("zellij")
        .args(new_tab_argv(session, cwd, tab, argv))
        .output()
        .map_err(|error| TabOpenError::Other(error.to_string()))?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    classify_new_tab(output.status.success(), &stderr)
}

pub(super) fn ensure_session(session: &str) -> Result<(), String> {
    let output = Command::new("zellij")
        .args(ensure_session_argv(session))
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn new_tab_argv(session: &str, cwd: &str, tab: &str, argv: &[String]) -> Vec<String> {
    let mut arguments = vec![
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
    arguments.extend(argv.iter().cloned());
    arguments
}

fn classify_new_tab(success: bool, stderr: &str) -> Result<(), TabOpenError> {
    if stderr.contains("not found") {
        return Err(TabOpenError::SessionNotFound);
    }
    if success {
        return Ok(());
    }
    Err(TabOpenError::Other(stderr.trim().to_string()))
}

fn ensure_session_argv(session: &str) -> Vec<String> {
    vec![
        "attach".to_string(),
        "--create-background".to_string(),
        session.to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use super::*;

    #[test]
    fn new_tab_argv_passes_command_through_after_double_dash() {
        let arguments = new_tab_argv(
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
            arguments,
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
        assert_eq!(arguments.last().unwrap(), "multi\nline prompt");
    }

    #[test]
    fn live_success_is_ok() {
        assert_matches!(classify_new_tab(true, ""), Ok(()));
    }

    #[test]
    fn exit_zero_not_found_is_a_missing_session() {
        let stderr = "Session 'ssh' not found. The following sessions are active:";

        assert_matches!(
            classify_new_tab(true, stderr),
            Err(TabOpenError::SessionNotFound)
        );
    }

    #[test]
    fn other_failures_retain_trimmed_provider_stderr() {
        assert_matches!(
            classify_new_tab(false, "  boom  "),
            Err(TabOpenError::Other(message)) if message == "boom"
        );
    }

    #[test]
    fn ensure_session_argv_creates_a_background_session() {
        assert_eq!(
            ensure_session_argv("cfg"),
            vec!["attach", "--create-background", "cfg"]
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>()
        );
    }
}
