#![cfg(test)]

//! Exercises user journeys through the release `pwf` binary.

#[path = "e2e/notes.rs"]
mod notes;
#[path = "e2e/project.rs"]
mod project;
#[cfg(unix)]
#[path = "e2e/session.rs"]
mod session;
#[allow(dead_code, unused_imports)]
#[path = "support/mod.rs"]
mod support;
#[path = "e2e/task.rs"]
mod task;
