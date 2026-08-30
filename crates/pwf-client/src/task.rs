use std::error::Error;

use pwf_wire::v1::{self, task_service_client::TaskServiceClient};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::Status;

use crate::{AuthenticatedChannel, ClientError, RequestPolicy};

const STREAM_BUFFER: usize = 2;

#[derive(Debug, Clone, PartialEq)]
pub enum Confirmation {
    DeleteTask(v1::DeleteTaskConfirmation),
    ReopenTask(v1::ReopenTaskConfirmation),
    DispatchSession(v1::SessionDispatchPreflight),
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

#[derive(Clone)]
pub struct TaskClient {
    channel: tonic::transport::Channel,
    request_policy: RequestPolicy,
}

impl TaskClient {
    pub(crate) fn new(channel: tonic::transport::Channel, request_policy: RequestPolicy) -> Self {
        Self {
            channel,
            request_policy,
        }
    }

    pub async fn create_task(
        &self,
        request: v1::CreateTaskRequest,
    ) -> Result<v1::CreatedTask, ClientError> {
        let response = self
            .client()
            .create_task(request)
            .await
            .map(tonic::Response::into_inner)
            .map_err(ClientError::from)?;
        required_response(response.task, "create response is missing its task")
    }

    pub async fn cancel_task(
        &self,
        request: v1::CancelTaskRequest,
    ) -> Result<v1::ClosedTask, ClientError> {
        let response = self
            .client()
            .cancel_task(request)
            .await
            .map(tonic::Response::into_inner)
            .map_err(ClientError::from)?;
        required_response(response.task, "cancel response is missing its task")
    }

    pub async fn complete_task(
        &self,
        request: v1::CompleteTaskRequest,
    ) -> Result<v1::ClosedTask, ClientError> {
        let response = self
            .client()
            .complete_task(request)
            .await
            .map(tonic::Response::into_inner)
            .map_err(ClientError::from)?;
        required_response(response.task, "complete response is missing its task")
    }

    pub async fn update_task(
        &self,
        request: v1::UpdateTaskRequest,
    ) -> Result<v1::UpdatedTask, ClientError> {
        let response = self
            .client()
            .update_task(request)
            .await
            .map(tonic::Response::into_inner)
            .map_err(ClientError::from)?;
        required_response(response.task, "update response is missing its task")
    }

    pub async fn get_task(
        &self,
        request: v1::GetTaskRequest,
    ) -> Result<v1::GetTaskResponse, ClientError> {
        self.client()
            .get_task(request)
            .await
            .map(tonic::Response::into_inner)
            .map_err(Into::into)
    }

    pub async fn list_tasks(
        &self,
        request: v1::ListTasksRequest,
    ) -> Result<v1::ListTasksResponse, ClientError> {
        self.client()
            .list_tasks(request)
            .await
            .map(tonic::Response::into_inner)
            .map_err(Into::into)
    }

    pub async fn plan_session(
        &self,
        request: v1::PlanSessionRequest,
    ) -> Result<v1::PlanSessionResponse, ClientError> {
        self.session_client()
            .plan_session(request)
            .await
            .map(tonic::Response::into_inner)
            .map_err(Into::into)
    }

    pub async fn delete_task<Prompt>(
        &self,
        request: v1::DeleteTaskStart,
        prompt: Prompt,
    ) -> Result<v1::DeleteTaskResult, ConfirmedRequestError<Prompt::Error>>
    where
        Prompt: ConfirmationPrompt,
    {
        let (sender, receiver) = mpsc::channel(STREAM_BUFFER);
        sender
            .send(v1::DeleteTaskRequest {
                value: Some(v1::delete_task_request::Value::Start(request)),
            })
            .await
            .map_err(|_| protocol("delete request stream closed"))?;
        let mut stream = self
            .client()
            .delete_task(ReceiverStream::new(receiver))
            .await
            .map_err(ConfirmedRequestError::Operation)?
            .into_inner();
        let first = stream
            .message()
            .await
            .map_err(ConfirmedRequestError::Operation)?
            .ok_or_else(|| protocol("delete response stream closed before preflight"))?;
        match first.value {
            Some(v1::delete_task_response::Value::Preflight(preflight)) => {
                let confirmed = prompt
                    .confirm(&Confirmation::DeleteTask(preflight))
                    .map_err(ConfirmedRequestError::Prompt)?;
                sender
                    .send(v1::DeleteTaskRequest {
                        value: Some(v1::delete_task_request::Value::Decision(
                            v1::ConfirmationDecision { confirmed },
                        )),
                    })
                    .await
                    .map_err(|_| protocol("delete decision stream closed"))?;
                let result = stream
                    .message()
                    .await
                    .map_err(ConfirmedRequestError::Operation)?
                    .ok_or_else(|| protocol("delete response stream closed before result"))?;
                match result.value {
                    Some(v1::delete_task_response::Value::Result(result)) => Ok(result),
                    Some(v1::delete_task_response::Value::Preflight(_)) | None => Err(protocol(
                        "delete response stream returned an invalid result",
                    )),
                }
            }
            Some(v1::delete_task_response::Value::Result(result)) => Ok(result),
            None => Err(protocol("delete response stream returned an empty message")),
        }
    }

    pub async fn reopen_task<Prompt>(
        &self,
        request: v1::ReopenTaskStart,
        prompt: Prompt,
    ) -> Result<v1::ReopenTaskResult, ConfirmedRequestError<Prompt::Error>>
    where
        Prompt: ConfirmationPrompt,
    {
        let (sender, receiver) = mpsc::channel(STREAM_BUFFER);
        sender
            .send(v1::ReopenTaskRequest {
                value: Some(v1::reopen_task_request::Value::Start(request)),
            })
            .await
            .map_err(|_| protocol("reopen request stream closed"))?;
        let mut stream = self
            .client()
            .reopen_task(ReceiverStream::new(receiver))
            .await
            .map_err(ConfirmedRequestError::Operation)?
            .into_inner();
        let first = stream
            .message()
            .await
            .map_err(ConfirmedRequestError::Operation)?
            .ok_or_else(|| protocol("reopen response stream closed before result"))?;
        match first.value {
            Some(v1::reopen_task_response::Value::Preflight(preflight)) => {
                let confirmed = prompt
                    .confirm(&Confirmation::ReopenTask(preflight))
                    .map_err(ConfirmedRequestError::Prompt)?;
                sender
                    .send(v1::ReopenTaskRequest {
                        value: Some(v1::reopen_task_request::Value::Decision(
                            v1::ConfirmationDecision { confirmed },
                        )),
                    })
                    .await
                    .map_err(|_| protocol("reopen decision stream closed"))?;
                let result = stream
                    .message()
                    .await
                    .map_err(ConfirmedRequestError::Operation)?
                    .ok_or_else(|| protocol("reopen response stream closed before result"))?;
                match result.value {
                    Some(v1::reopen_task_response::Value::Result(result)) => Ok(result),
                    Some(v1::reopen_task_response::Value::Preflight(_)) | None => Err(protocol(
                        "reopen response stream returned an invalid result",
                    )),
                }
            }
            Some(v1::reopen_task_response::Value::Result(result)) => Ok(result),
            None => Err(protocol("reopen response stream returned an empty message")),
        }
    }

    pub async fn dispatch_session<Prompt>(
        &self,
        request: v1::PlanSessionRequest,
        prompt: Prompt,
    ) -> Result<v1::DispatchedSession, ConfirmedRequestError<Prompt::Error>>
    where
        Prompt: ConfirmationPrompt,
    {
        let (sender, receiver) = mpsc::channel(STREAM_BUFFER);
        sender
            .send(v1::DispatchSessionRequest {
                value: Some(v1::dispatch_session_request::Value::Start(request)),
            })
            .await
            .map_err(|_| protocol("session request stream closed"))?;
        let mut stream = self
            .session_client()
            .dispatch_session(ReceiverStream::new(receiver))
            .await
            .map_err(ConfirmedRequestError::Operation)?
            .into_inner();
        let first = stream
            .message()
            .await
            .map_err(ConfirmedRequestError::Operation)?
            .ok_or_else(|| protocol("session response stream closed before preflight"))?;
        match first.value {
            Some(v1::dispatch_session_response::Value::Preflight(preflight)) => {
                let confirmed = prompt
                    .confirm(&Confirmation::DispatchSession(preflight))
                    .map_err(ConfirmedRequestError::Prompt)?;
                sender
                    .send(v1::DispatchSessionRequest {
                        value: Some(v1::dispatch_session_request::Value::Decision(
                            v1::ConfirmationDecision { confirmed },
                        )),
                    })
                    .await
                    .map_err(|_| protocol("session decision stream closed"))?;
                let result = stream
                    .message()
                    .await
                    .map_err(ConfirmedRequestError::Operation)?
                    .ok_or_else(|| protocol("session response stream closed before result"))?;
                match result.value {
                    Some(v1::dispatch_session_response::Value::Result(result)) => Ok(result),
                    Some(v1::dispatch_session_response::Value::Preflight(_)) | None => Err(
                        protocol("session response stream returned an invalid result"),
                    ),
                }
            }
            Some(v1::dispatch_session_response::Value::Result(result)) => Ok(result),
            None => Err(protocol(
                "session response stream returned an empty message",
            )),
        }
    }

    fn client(&self) -> TaskServiceClient<AuthenticatedChannel> {
        TaskServiceClient::with_interceptor(self.channel.clone(), self.request_policy.clone())
            .max_encoding_message_size(super::MAX_REQUEST_MESSAGE_SIZE)
            .max_decoding_message_size(super::MAX_RESPONSE_MESSAGE_SIZE)
    }

    fn session_client(
        &self,
    ) -> v1::session_service_client::SessionServiceClient<AuthenticatedChannel> {
        v1::session_service_client::SessionServiceClient::with_interceptor(
            self.channel.clone(),
            self.request_policy.clone(),
        )
        .max_encoding_message_size(super::MAX_REQUEST_MESSAGE_SIZE)
        .max_decoding_message_size(super::MAX_RESPONSE_MESSAGE_SIZE)
    }
}

fn protocol<PromptError>(message: &'static str) -> ConfirmedRequestError<PromptError>
where
    PromptError: Error + 'static,
{
    ConfirmedRequestError::Operation(Status::internal(message))
}

fn required_response<T>(value: Option<T>, message: &'static str) -> Result<T, ClientError> {
    value.ok_or_else(|| ClientError::from(Status::internal(message)))
}
