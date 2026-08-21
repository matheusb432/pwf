//! Exposes pwf command parsing and shared CLI support.

pub mod command;
mod confirmation;
pub mod console;
pub mod note;
mod preprocess;
pub mod project;
pub mod task;

pub(crate) fn rpc_error(error: pwf_client::ClientError) -> anyhow::Error {
    match error {
        pwf_client::ClientError::Rpc(status) => anyhow::anyhow!(status.message().to_string()),
    }
}
