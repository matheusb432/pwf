pub use pwf_wire::task::{TaskDag, TaskDagEdge, TaskDagError, TaskDagNode};
use pwf_wire::{
    pb::{self, task_service_client::TaskServiceClient},
    proto::task::decode_get_task_dag_response,
};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

use crate::{
    ClientError, PolicyChannel, RequestPolicy,
    confirmation::{Confirmation, ConfirmationPrompt, ConfirmedRequestError, protocol},
};

const STREAM_BUFFER: usize = 2;

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
        mut request: pb::CreateTaskRequest,
    ) -> Result<pb::CreateTaskResponse, ClientError> {
        ensure_request_id(&mut request.request_id);
        self.client()
            .create_task(request)
            .await
            .map(tonic::Response::into_inner)
            .map_err(ClientError::from)
    }

    pub async fn cancel_task(
        &self,
        mut request: pb::CancelTaskRequest,
    ) -> Result<pb::CancelTaskResponse, ClientError> {
        ensure_request_id(&mut request.request_id);
        self.client()
            .cancel_task(request)
            .await
            .map(tonic::Response::into_inner)
            .map_err(ClientError::from)
    }

    pub async fn complete_task(
        &self,
        mut request: pb::CompleteTaskRequest,
    ) -> Result<pb::CompleteTaskResponse, ClientError> {
        ensure_request_id(&mut request.request_id);
        self.client()
            .complete_task(request)
            .await
            .map(tonic::Response::into_inner)
            .map_err(ClientError::from)
    }

    pub async fn update_task(
        &self,
        mut request: pb::UpdateTaskRequest,
    ) -> Result<pb::UpdateTaskResponse, ClientError> {
        ensure_request_id(&mut request.request_id);
        self.client()
            .update_task(request)
            .await
            .map(tonic::Response::into_inner)
            .map_err(ClientError::from)
    }

    pub async fn get_task(
        &self,
        request: pb::GetTaskRequest,
    ) -> Result<pb::GetTaskResponse, ClientError> {
        self.client()
            .get_task(request)
            .await
            .map(tonic::Response::into_inner)
            .map_err(Into::into)
    }

    pub async fn get_task_dag(
        &self,
        request: pb::GetTaskDagRequest,
    ) -> Result<TaskDag, ClientError> {
        let response = self
            .client()
            .get_task_dag(request)
            .await
            .map(tonic::Response::into_inner)
            .map_err(ClientError::from)?;
        decode_get_task_dag_response(response).map_err(Into::into)
    }

    pub async fn list_tasks(
        &self,
        request: pb::ListTasksRequest,
    ) -> Result<pb::ListTasksResponse, ClientError> {
        self.client()
            .list_tasks(request)
            .await
            .map(tonic::Response::into_inner)
            .map_err(Into::into)
    }

    pub async fn plan_session(
        &self,
        request: pb::PlanSessionRequest,
    ) -> Result<pb::PlanSessionResponse, ClientError> {
        self.session_client()
            .plan_session(request)
            .await
            .map(tonic::Response::into_inner)
            .map_err(Into::into)
    }

    pub async fn delete_task<Prompt>(
        &self,
        mut request: pb::DeleteTaskStart,
        prompt: Prompt,
    ) -> Result<pb::DeleteTaskResult, ConfirmedRequestError<Prompt::Error>>
    where
        Prompt: ConfirmationPrompt,
    {
        ensure_request_id(&mut request.request_id);
        let (sender, receiver) = mpsc::channel(STREAM_BUFFER);
        sender
            .send(pb::DeleteTaskRequest {
                value: Some(pb::delete_task_request::Value::Start(request)),
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
            Some(pb::delete_task_response::Value::Preflight(preflight)) => {
                let confirmation = preflight
                    .confirmation
                    .ok_or_else(|| protocol("delete preflight is missing its confirmation"))?;
                let confirmed = prompt
                    .confirm(&Confirmation::DeleteTask(confirmation))
                    .map_err(ConfirmedRequestError::Prompt)?;
                sender
                    .send(pb::DeleteTaskRequest {
                        value: Some(pb::delete_task_request::Value::Decision(
                            pb::ConfirmationDecision { confirmed },
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
                    Some(pb::delete_task_response::Value::Result(result)) => Ok(result),
                    Some(pb::delete_task_response::Value::Preflight(_)) | None => Err(protocol(
                        "delete response stream returned an invalid result",
                    )),
                }
            }
            Some(pb::delete_task_response::Value::Result(result)) => Ok(result),
            None => Err(protocol("delete response stream returned an empty message")),
        }
    }

    pub async fn reopen_task<Prompt>(
        &self,
        mut request: pb::ReopenTaskStart,
        prompt: Prompt,
    ) -> Result<pb::ReopenTaskResult, ConfirmedRequestError<Prompt::Error>>
    where
        Prompt: ConfirmationPrompt,
    {
        ensure_request_id(&mut request.request_id);
        let (sender, receiver) = mpsc::channel(STREAM_BUFFER);
        sender
            .send(pb::ReopenTaskRequest {
                value: Some(pb::reopen_task_request::Value::Start(request)),
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
            Some(pb::reopen_task_response::Value::Preflight(preflight)) => {
                let confirmation = preflight
                    .confirmation
                    .ok_or_else(|| protocol("reopen preflight is missing its confirmation"))?;
                let confirmed = prompt
                    .confirm(&Confirmation::ReopenTask(confirmation))
                    .map_err(ConfirmedRequestError::Prompt)?;
                sender
                    .send(pb::ReopenTaskRequest {
                        value: Some(pb::reopen_task_request::Value::Decision(
                            pb::ConfirmationDecision { confirmed },
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
                    Some(pb::reopen_task_response::Value::Result(result)) => Ok(result),
                    Some(pb::reopen_task_response::Value::Preflight(_)) | None => Err(protocol(
                        "reopen response stream returned an invalid result",
                    )),
                }
            }
            Some(pb::reopen_task_response::Value::Result(result)) => Ok(result),
            None => Err(protocol("reopen response stream returned an empty message")),
        }
    }

    pub async fn dispatch_session<Prompt>(
        &self,
        request: pb::DispatchSessionStart,
        prompt: Prompt,
    ) -> Result<pb::DispatchedSession, ConfirmedRequestError<Prompt::Error>>
    where
        Prompt: ConfirmationPrompt,
    {
        let (sender, receiver) = mpsc::channel(STREAM_BUFFER);
        sender
            .send(pb::DispatchSessionRequest {
                value: Some(pb::dispatch_session_request::Value::Start(request)),
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
            Some(pb::dispatch_session_response::Value::Preflight(preflight)) => {
                let confirmed = prompt
                    .confirm(&Confirmation::DispatchSession(preflight))
                    .map_err(ConfirmedRequestError::Prompt)?;
                sender
                    .send(pb::DispatchSessionRequest {
                        value: Some(pb::dispatch_session_request::Value::Decision(
                            pb::ConfirmationDecision { confirmed },
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
                    Some(pb::dispatch_session_response::Value::Result(result)) => Ok(result),
                    Some(pb::dispatch_session_response::Value::Preflight(_)) | None => Err(
                        protocol("session response stream returned an invalid result"),
                    ),
                }
            }
            Some(pb::dispatch_session_response::Value::Result(result)) => Ok(result),
            None => Err(protocol(
                "session response stream returned an empty message",
            )),
        }
    }

    fn client(&self) -> TaskServiceClient<PolicyChannel> {
        TaskServiceClient::with_interceptor(
            crate::release::ReleaseChannel(self.channel.clone()),
            self.request_policy,
        )
        .max_encoding_message_size(super::MAX_REQUEST_MESSAGE_SIZE)
        .max_decoding_message_size(super::MAX_RESPONSE_MESSAGE_SIZE)
    }

    fn session_client(&self) -> pb::session_service_client::SessionServiceClient<PolicyChannel> {
        pb::session_service_client::SessionServiceClient::with_interceptor(
            crate::release::ReleaseChannel(self.channel.clone()),
            self.request_policy,
        )
        .max_encoding_message_size(super::MAX_REQUEST_MESSAGE_SIZE)
        .max_decoding_message_size(super::MAX_RESPONSE_MESSAGE_SIZE)
    }
}

fn ensure_request_id(request_id: &mut String) {
    if request_id.is_empty() {
        *request_id = Uuid::new_v4().to_string();
    }
}
