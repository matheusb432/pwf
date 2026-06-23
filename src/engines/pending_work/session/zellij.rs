//! Zellij driver seam: the side-effecting calls live behind `ZellijDriver`; the
//! command vectors are built by pure functions so the contract is unit-tested
//! without spawning zellij.

use std::process::Command;

/// Why a `new-tab` attempt failed.
#[derive(Debug)]
pub(in crate::engines::pending_work) enum NewTabError {
    /// Zellij reported the target session is not running.
    SessionNotFound,
    /// Any other failure (stderr text carried for the error message).
    Other(String),
}

impl std::fmt::Display for NewTabError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NewTabError::SessionNotFound => write!(f, "session not found"),
            NewTabError::Other(m) => write!(f, "{m}"),
        }
    }
}

/// Side-effecting zellij operations, mockable in tests.
pub(in crate::engines::pending_work) trait ZellijDriver {
    /// `zellij` resolves and runs (preflight for `ZellijNotFound`).
    fn available(&self) -> bool;
    /// Add a tab named `tab` to running `session`, cwd `cwd`, running `argv`.
    fn new_tab(
        &self,
        session: &str,
        cwd: &str,
        tab: &str,
        argv: &[String],
    ) -> Result<(), NewTabError>;
    /// Create/resurrect `session` headlessly so a later `new_tab` can target it.
    fn ensure_session(&self, session: &str) -> Result<(), String>;
}

/// `zellij --session <s> action new-tab --cwd <cwd> --name <tab> -- <argv…>`.
pub(in crate::engines::pending_work) fn new_tab_argv(
    session: &str,
    cwd: &str,
    tab: &str,
    argv: &[String],
) -> Vec<String> {
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

/// `zellij attach --create-background <session>`.
pub(in crate::engines::pending_work) fn ensure_session_argv(session: &str) -> Vec<String> {
    vec![
        "attach".to_string(),
        "--create-background".to_string(),
        session.to_string(),
    ]
}

#[cfg(test)]
mod tests {
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

/// Real driver: shells out to the `zellij` binary.
pub(in crate::engines::pending_work) struct RealZellij;

impl ZellijDriver for RealZellij {
    fn available(&self) -> bool {
        Command::new("zellij")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
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
        if output.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("not found") {
            Err(NewTabError::SessionNotFound)
        } else {
            Err(NewTabError::Other(stderr.trim().to_string()))
        }
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
pub(in crate::engines::pending_work) mod fake {
    use super::*;
    use std::cell::RefCell;

    /// Scriptable driver. `new_tab_results` is consumed front-to-back per call.
    pub(in crate::engines::pending_work) struct FakeZellij {
        pub available: bool,
        pub new_tab_results: RefCell<Vec<Result<(), NewTabError>>>,
        pub ensure_result: Result<(), String>,
        pub calls: RefCell<Vec<String>>,
    }

    impl FakeZellij {
        pub fn new(available: bool, new_tab_results: Vec<Result<(), NewTabError>>) -> Self {
            Self {
                available,
                new_tab_results: RefCell::new(new_tab_results),
                ensure_result: Ok(()),
                calls: RefCell::new(vec![]),
            }
        }
    }

    impl ZellijDriver for FakeZellij {
        fn available(&self) -> bool {
            self.available
        }
        fn new_tab(
            &self,
            session: &str,
            _cwd: &str,
            tab: &str,
            _argv: &[String],
        ) -> Result<(), NewTabError> {
            self.calls
                .borrow_mut()
                .push(format!("new_tab:{session}:{tab}"));
            self.new_tab_results.borrow_mut().remove(0)
        }
        fn ensure_session(&self, session: &str) -> Result<(), String> {
            self.calls.borrow_mut().push(format!("ensure:{session}"));
            self.ensure_result.clone()
        }
    }
}
