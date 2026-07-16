//! Inline dispatch backend: run the agent in the CURRENT terminal instead of a
//! multiplexer tab. On Unix the pwf process is replaced (execvp) so the agent
//! inherits the real controlling terminal; elsewhere we spawn-and-wait. The
//! side-effecting call lives behind `InlineExec` so orchestration is unit-tested
//! with a fake that records the argv and cwd — nothing is spawned in tests.

use std::process::Command;

/// Run an agent `argv` in the current terminal, working directory `cwd`.
pub(in crate::engines::pending_work) trait InlineExec {
    /// On Unix, replace this process image with `argv` (execvp) — only returns on
    /// failure (`Err`); on success the process is gone. On non-Unix, spawn the
    /// agent inheriting stdio, wait, and map a non-zero exit to `Err`.
    fn run(&self, argv: &[String], cwd: &str) -> Result<(), String>;
}

/// Real backend: execs (Unix) or spawns-and-waits (non-Unix).
pub(in crate::engines::pending_work) struct RealExec;

impl InlineExec for RealExec {
    fn run(&self, argv: &[String], cwd: &str) -> Result<(), String> {
        let (bin, rest) = argv.split_first().ok_or_else(|| "empty argv".to_string())?;
        let mut cmd = Command::new(bin);
        cmd.args(rest).current_dir(cwd);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // exec() returns only on failure; on success the image is replaced and
            // the agent's exit code becomes pwf's.
            Err(cmd.exec().to_string())
        }
        #[cfg(not(unix))]
        {
            let status = cmd.status().map_err(|e| e.to_string())?;
            if status.success() {
                Ok(())
            } else {
                Err(format!(
                    "agent exited with status {}",
                    status.code().unwrap_or(-1)
                ))
            }
        }
    }
}
