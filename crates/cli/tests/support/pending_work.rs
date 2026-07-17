//! Runs pending-work operations in-process for integration tests.

use pwf::engines::pending_work;

pub(super) fn run_args_plain(args: &pwf::cli::EngineArgs) -> Result<String, String> {
    pending_work::run_args(args).map(|stdout| normalize_stdout(&stdout))
}

pub(super) fn normalize_stdout(stdout: &str) -> String {
    console::strip_ansi_codes(stdout).into_owned()
}
