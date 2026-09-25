use tonic::Status;

use crate::{DecodeGetTaskDagResponseError, DecodeGetTaskResponseError};

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("pwf-server returned an invalid task: {0}")]
    InvalidTaskResponse(#[from] DecodeGetTaskResponseError),
    #[error("pwf-server request failed: {0}")]
    Rpc(#[from] Status),
    #[error("pwf-server returned an invalid task dependency graph: {0}")]
    InvalidTaskDagResponse(#[from] DecodeGetTaskDagResponseError),
}

impl ClientError {
    #[must_use]
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::Rpc(status) if status.code() == tonic::Code::NotFound)
    }

    /// Reports whether the server and this client rejected each other's release versions.
    #[must_use]
    pub fn is_release_mismatch(&self) -> bool {
        matches!(
            self,
            Self::Rpc(status) if status.code() == tonic::Code::FailedPrecondition
                && status
                    .metadata()
                    .get("pwf-server-version")
                    .is_some_and(|version| version.to_str().ok() != Some(env!("CARGO_PKG_VERSION")))
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rejection(code: tonic::Code, server_version: &'static str) -> ClientError {
        let mut status = Status::new(code, "rejected");
        status.metadata_mut().insert(
            "pwf-server-version",
            tonic::metadata::MetadataValue::from_static(server_version),
        );
        ClientError::Rpc(status)
    }

    #[test]
    fn release_mismatch_requires_a_different_server_version() {
        assert!(rejection(tonic::Code::FailedPrecondition, "0.0.0-other").is_release_mismatch());
        assert!(
            !rejection(tonic::Code::FailedPrecondition, env!("CARGO_PKG_VERSION"))
                .is_release_mismatch()
        );
        assert!(!rejection(tonic::Code::NotFound, "0.0.0-other").is_release_mismatch());
    }
}
