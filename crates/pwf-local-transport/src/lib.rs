//! User-private local IPC for the CLI and bundled server.

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub use unix::{LocalEndpoint, listener::LocalListener};
#[cfg(windows)]
pub use windows::{LocalEndpoint, LocalListener};

#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    #[error("cannot access the local pwf-server endpoint")]
    LocalEndpoint(#[from] std::io::Error),
    #[error("could not connect to local pwf-server")]
    Transport(#[from] tonic::transport::Error),
}
