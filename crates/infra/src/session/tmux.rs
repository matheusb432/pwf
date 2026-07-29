//! Owns the tmux command protocol for session windows.

use std::process::{Command, Output};

use pwf_application::pending_work::session::TmuxSessionClient;

#[derive(Debug, Clone, Copy, Default)]
pub struct TmuxHarness;

impl TmuxHarness {
    #[must_use]
    pub fn available() -> bool {
        Command::new("tmux")
            .arg("-V")
            .output()
            .is_ok_and(|output| output.status.success())
    }

    pub fn session_exists(session: &str) -> Result<bool, String> {
        let output = Command::new("tmux")
            .args(["has-session", "-t", &exact_session(session)])
            .output()
            .map_err(|error| error.to_string())?;
        classify_session_exists(&output)
    }

    #[must_use]
    pub fn new_window_process_argv(
        session: &str,
        repository: &str,
        window: &str,
        argv: &[String],
    ) -> Vec<String> {
        let mut process_argv = vec![
            "tmux".to_string(),
            "new-window".to_string(),
            "-d".to_string(),
            "-t".to_string(),
            format!("{}:", exact_session(session)),
            "-c".to_string(),
            repository.to_string(),
            "-n".to_string(),
            window.to_string(),
            "--".to_string(),
        ];
        process_argv.extend(argv.iter().cloned());
        process_argv
    }

    #[must_use]
    pub fn new_session_process_argv(session: &str, repository: &str) -> Vec<String> {
        vec![
            "tmux".to_string(),
            "new-session".to_string(),
            "-d".to_string(),
            "-s".to_string(),
            session.to_string(),
            "-c".to_string(),
            repository.to_string(),
        ]
    }

    pub fn open_window(
        session: &str,
        repository: &str,
        window: &str,
        argv: &[String],
    ) -> Result<(), String> {
        let process_argv = Self::new_window_process_argv(session, repository, window, argv);
        let output = Command::new(&process_argv[0])
            .args(&process_argv[1..])
            .output()
            .map_err(|error| error.to_string())?;
        if output.status.success() {
            Ok(())
        } else {
            Err(output_error(&output))
        }
    }
}

impl TmuxSessionClient for TmuxHarness {
    fn available(&self) -> bool {
        Self::available()
    }

    fn session_exists(&self, session: &str) -> Result<bool, String> {
        Self::session_exists(session)
    }

    fn new_window_process_argv(
        &self,
        session: &str,
        repository: &str,
        window: &str,
        argv: &[String],
    ) -> Vec<String> {
        Self::new_window_process_argv(session, repository, window, argv)
    }

    fn new_session_process_argv(&self, session: &str, repository: &str) -> Vec<String> {
        Self::new_session_process_argv(session, repository)
    }

    fn open_window(
        &self,
        session: &str,
        repository: &str,
        window: &str,
        argv: &[String],
    ) -> Result<(), String> {
        Self::open_window(session, repository, window, argv)
    }
}

fn exact_session(session: &str) -> String {
    format!("={session}")
}

fn classify_session_exists(output: &Output) -> Result<bool, String> {
    if output.status.success() {
        return Ok(true);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("can't find session") || stderr.contains("no server running") {
        return Ok(false);
    }
    Err(output_error(output))
}

fn output_error(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let message = stderr.trim();
    if message.is_empty() {
        format!("tmux exited with {}", output.status)
    } else {
        message.to_string()
    }
}
