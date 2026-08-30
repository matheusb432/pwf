use std::error::Error;

use pwf_wire::pb;
use tonic::Status;

#[derive(Debug, Clone, PartialEq)]
pub enum Confirmation {
    DeleteNote(pb::DeleteNoteConfirmation),
    DeleteTask(pb::DeleteTaskConfirmation),
    ReopenTask(pb::ReopenTaskConfirmation),
    DispatchSession(pb::SessionDispatchPreflight),
}

pub trait ConfirmationPrompt: Send + Sync + 'static {
    type Error: Error + Send + Sync + 'static;

    fn confirm(&self, confirmation: &Confirmation) -> Result<bool, Self::Error>;
}

#[derive(Debug, thiserror::Error)]
pub enum ConfirmedRequestError<PromptError>
where
    PromptError: Error + 'static,
{
    #[error(transparent)]
    Operation(Status),
    #[error(transparent)]
    Prompt(PromptError),
}

pub(crate) fn protocol<PromptError>(message: &'static str) -> ConfirmedRequestError<PromptError>
where
    PromptError: Error + 'static,
{
    ConfirmedRequestError::Operation(Status::internal(message))
}
