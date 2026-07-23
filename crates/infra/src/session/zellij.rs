//! Owns the Zellij command protocol and tab-open failure classification.

use std::process::Command;

use pwf_application::pending_work::session::{ZellijSessionClient, ZellijTabOpenError};

/// Executes prepared agent commands in Zellij.
#[derive(Debug, Clone, Copy, Default)]
pub struct ZellijHarness;

impl ZellijHarness {
    /// Reports whether Zellij is available on the current process path.
    #[must_use]
    pub fn available() -> bool {
        Command::new("zellij")
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
    }

    /// Opens prepared argv in a named Zellij tab.
    ///
    /// # Errors
    ///
    /// Returns [`ZellijTabOpenError::SessionNotFound`] when the session is absent and
    /// [`ZellijTabOpenError::Rejected`] for every other provider rejection.
    pub fn open_tab(
        session: &str,
        repository: &str,
        tab: &str,
        argv: &[String],
    ) -> Result<(), ZellijTabOpenError> {
        let process_argv = Self::new_tab_process_argv(session, repository, tab, argv);
        let output = Command::new(&process_argv[0])
            .args(&process_argv[1..])
            .output()
            .map_err(|error| ZellijTabOpenError::Rejected(error.to_string()))?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        classify_new_tab(output.status.success(), &stderr)
    }

    /// Returns the complete zellij process argv for a named tab.
    #[must_use]
    pub fn new_tab_process_argv(
        session: &str,
        repository: &str,
        tab: &str,
        argv: &[String],
    ) -> Vec<String> {
        let mut arguments = vec![
            "zellij".to_string(),
            "--session".to_string(),
            session.to_string(),
            "action".to_string(),
            "new-tab".to_string(),
            "--cwd".to_string(),
            repository.to_string(),
            "--name".to_string(),
            tab.to_string(),
            "--".to_string(),
        ];
        arguments.extend(argv.iter().cloned());
        arguments
    }

    /// Creates or resurrects a named Zellij session.
    ///
    /// # Errors
    ///
    /// Returns the provider message when the session cannot be created or restored.
    pub fn ensure_session(session: &str) -> Result<(), String> {
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
}

impl ZellijSessionClient for ZellijHarness {
    fn available(&self) -> bool {
        Self::available()
    }

    fn new_tab_process_argv(
        &self,
        session: &str,
        repository: &str,
        tab: &str,
        argv: &[String],
    ) -> Vec<String> {
        Self::new_tab_process_argv(session, repository, tab, argv)
    }

    fn open_tab(
        &self,
        session: &str,
        repository: &str,
        tab: &str,
        argv: &[String],
    ) -> Result<(), ZellijTabOpenError> {
        Self::open_tab(session, repository, tab, argv)
    }

    fn ensure_session(&self, session: &str) -> Result<(), String> {
        Self::ensure_session(session)
    }
}

fn classify_new_tab(success: bool, stderr: &str) -> Result<(), ZellijTabOpenError> {
    if stderr.contains("not found") {
        return Err(ZellijTabOpenError::SessionNotFound);
    }
    if success {
        return Ok(());
    }
    Err(ZellijTabOpenError::Rejected(stderr.trim().to_string()))
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

    use super::{ZellijTabOpenError, classify_new_tab};

    #[test]
    fn tab_result_classifies_missing_session_and_provider_rejection() {
        assert_matches!(classify_new_tab(true, ""), Ok(()));
        assert_matches!(
            classify_new_tab(
                true,
                "Session 'ssh' not found. The following sessions are active:"
            ),
            Err(ZellijTabOpenError::SessionNotFound)
        );
        assert_matches!(
            classify_new_tab(false, "  boom  "),
            Err(ZellijTabOpenError::Rejected(message)) if message == "boom"
        );
    }
}
