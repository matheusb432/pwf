//! Exposes pwf command parsing and shared CLI support.

pub mod command;
mod entrypoint;
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

pub(crate) fn rpc_error(error: pwf_client::ClientError) -> anyhow::Error {
    match error {
        pwf_client::ClientError::Rpc(status) => anyhow::anyhow!(status.message().to_string()),
        pwf_client::ClientError::InvalidTaskDagResponse(error) => anyhow::Error::new(error),
    }
}
