//! Owns the tmux command protocol for session windows.

use std::process::{Command, Output};

use pwf_application::{SessionClient, SessionStart, SessionWindow};

#[derive(Debug, Clone, Copy, Default)]
pub struct TmuxHarness;

impl SessionClient for TmuxHarness {
    fn available(&self) -> bool {
        Command::new("tmux")
            .arg("-V")
            .output()
            .is_ok_and(|output| output.status.success())
    }

    fn session_exists(&self, session_name: &str) -> Result<bool, String> {
        let output = Command::new("tmux")
            .args(["has-session", "-t", &exact_session(session_name)])
            .output()
            .map_err(|error| error.to_string())?;
        classify_session_exists(&output)
    }

    fn preview_start(&self, start: &SessionStart<'_>) -> Vec<String> {
        vec![
            "tmux".to_string(),
            "new-session".to_string(),
            "-d".to_string(),
            "-s".to_string(),
            start.session_name().to_string(),
            "-c".to_string(),
            start.working_directory().to_string(),
        ]
    }

    fn preview_window(&self, window: &SessionWindow<'_>) -> Vec<String> {
        let mut process_arguments = vec![
            "tmux".to_string(),
            "new-window".to_string(),
            "-d".to_string(),
            "-t".to_string(),
            format!("{}:", exact_session(window.session_name())),
            "-c".to_string(),
            window.working_directory().to_string(),
            "-n".to_string(),
            window.window_name().to_string(),
            "--".to_string(),
        ];
        process_arguments.extend(window.agent_command().arguments().iter().cloned());
        process_arguments
    }

    fn open_window(&self, window: &SessionWindow<'_>) -> Result<(), String> {
        let process_arguments = self.preview_window(window);
        let output = Command::new(&process_arguments[0])
            .args(&process_arguments[1..])
            .output()
            .map_err(|error| error.to_string())?;
        if output.status.success() {
            Ok(())
        } else {
            Err(output_error(&output))
        }
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
