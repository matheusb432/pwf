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
}
