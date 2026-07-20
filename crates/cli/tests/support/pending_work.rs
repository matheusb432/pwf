//! Runs pending-work operations in-process for integration tests.

use pwf::engines::pending_work;

pub(super) fn run_plain(command: &pending_work::Command) -> Result<String, String> {
    pending_work::run(command).map(|stdout| normalize_stdout(&stdout))
}

pub(super) fn normalize_stdout(stdout: &str) -> String {
    console::strip_ansi_codes(stdout).into_owned()
}
