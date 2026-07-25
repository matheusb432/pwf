//! Runs pending-work operations in-process for integration tests.

use pwf::{console::Console, engines::pending_work};

use crate::projects::TestProjects;

/// Runs the command with a non-interactive, plain-markdown console, so results
/// stay deterministic whether or not the test process is attached to a TTY.
pub(super) fn run_plain(
    command: &pending_work::Command,
    projects: &TestProjects,
) -> Result<String, String> {
    pending_work::run(
        command,
        Console::plain(),
        &projects.store,
        &projects.registry,
    )
}
