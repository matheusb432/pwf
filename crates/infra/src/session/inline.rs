//! Executes prepared agent commands in the current terminal.

use std::process::Command;

use pwf_application::pending_work::session::InlineSessionClient;

/// Executes prepared agent commands inline.
#[derive(Debug, Clone, Copy, Default)]
pub struct InlineHarness;

impl InlineHarness {
    /// Runs prepared argv in the supplied repository.
    ///
    /// # Errors
    ///
    /// Returns a process error or unsuccessful exit status.
    pub fn run(argv: &[String], repository: &str) -> Result<(), String> {
        let (binary, arguments) = argv.split_first().ok_or_else(|| "empty argv".to_string())?;
        let mut command = Command::new(binary);
        command.args(arguments).current_dir(repository);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            Err(command.exec().to_string())
        }
        #[cfg(not(unix))]
        {
            let status = command.status().map_err(|error| error.to_string())?;
            map_spawn_status(status.success(), status.code())
        }
    }
}

impl InlineSessionClient for InlineHarness {
    fn run(&self, argv: &[String], repository: &str) -> Result<(), String> {
        Self::run(argv, repository)
    }
}

#[cfg(any(not(unix), test))]
fn map_spawn_status(success: bool, code: Option<i32>) -> Result<(), String> {
    if success {
        Ok(())
    } else {
        Err(format!("agent exited with status {}", code.unwrap_or(-1)))
    }
}

#[cfg(test)]
mod tests {
    use super::map_spawn_status;

    #[test]
    fn unsuccessful_status_retains_the_exit_classification() {
        assert_eq!(
            map_spawn_status(false, Some(17)),
            Err("agent exited with status 17".to_string())
        );
        assert_eq!(
            map_spawn_status(false, None),
            Err("agent exited with status -1".to_string())
        );
    }
}
