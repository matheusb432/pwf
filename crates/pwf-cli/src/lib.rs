//! Exposes pwf command parsing and shared CLI support.

pub mod command;
pub mod data;
pub mod doctor;
mod entrypoint;
pub mod error;
pub use entrypoint::run;
mod confirmation;
pub mod console;
mod edit;
pub mod note;
pub mod project;
mod render;
pub mod server;
pub mod settings;
pub mod task;
#[cfg(test)]
#[path = "../tests/support/style.rs"]
mod test_style;

pub(crate) use error::rpc_error;
