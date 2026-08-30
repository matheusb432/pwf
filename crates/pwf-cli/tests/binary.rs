#![cfg(test)]

//! Exercises public CLI contracts through the release `pwf` binary.

#[path = "binary/migrator.rs"]
mod migrator;
#[path = "binary/notes.rs"]
mod notes;
#[path = "binary/project_edit.rs"]
mod project_edit;
#[path = "binary/project_registry.rs"]
mod project_registry;
#[path = "binary/project_rename.rs"]
mod project_rename;
#[path = "binary/session_confirmation.rs"]
mod session_confirmation;
#[path = "binary/session_dispatch.rs"]
mod session_dispatch;
#[allow(dead_code, unused_imports)]
#[path = "support/mod.rs"]
mod support;
#[path = "binary/task.rs"]
mod task;
