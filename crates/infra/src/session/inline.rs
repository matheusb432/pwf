//! Executes a prepared agent command in the current terminal.

use std::process::Command;

pub(super) fn run(argv: &[String], cwd: &str) -> Result<(), String> {
    let (binary, arguments) = argv.split_first().ok_or_else(|| "empty argv".to_string())?;
    let mut command = Command::new(binary);
    command.args(arguments).current_dir(cwd);
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
    use super::*;

    #[test]
    fn empty_argv_is_rejected() {
        assert_eq!(run(&[], "."), Err("empty argv".to_string()));
    }

    #[test]
    fn successful_spawn_status_maps_to_success() {
        assert_eq!(map_spawn_status(true, Some(0)), Ok(()));
    }

    #[test]
    fn failed_spawn_status_retains_the_exit_code() {
        assert_eq!(
            map_spawn_status(false, Some(17)),
            Err("agent exited with status 17".to_string())
        );
    }

    #[test]
    fn signal_termination_maps_to_negative_one() {
        assert_eq!(
            map_spawn_status(false, None),
            Err("agent exited with status -1".to_string())
        );
    }
}
