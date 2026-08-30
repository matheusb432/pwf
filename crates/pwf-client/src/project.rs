use pwf_wire::pb::{self, project_service_client::ProjectServiceClient};

use crate::{AuthenticatedChannel, ClientError, RequestPolicy};

#[derive(Clone)]
pub struct ProjectClient {
    channel: tonic::transport::Channel,
    request_policy: RequestPolicy,
}

impl ProjectClient {
    pub(crate) fn new(channel: tonic::transport::Channel, request_policy: RequestPolicy) -> Self {
        Self {
            channel,
            request_policy,
        }
    }

    pub async fn add_project(
        &self,
        request: pb::AddProjectRequest,
    ) -> Result<pb::AddProjectResponse, ClientError> {
        self.client()
            .add_project(request)
            .await
            .map(tonic::Response::into_inner)
            .map_err(Into::into)
    }

    pub async fn get_project(
        &self,
        request: pb::GetProjectRequest,
    ) -> Result<pb::GetProjectResponse, ClientError> {
        self.client()
            .get_project(request)
            .await
            .map(tonic::Response::into_inner)
            .map_err(Into::into)
    }

    pub async fn list_projects(
        &self,
        request: pb::ListProjectsRequest,
    ) -> Result<pb::ListProjectsResponse, ClientError> {
        self.client()
            .list_projects(request)
            .await
            .map(tonic::Response::into_inner)
            .map_err(Into::into)
    }

    pub async fn pause_project(
        &self,
        request: pb::PauseProjectRequest,
    ) -> Result<pb::PauseProjectResponse, ClientError> {
        self.client()
            .pause_project(request)
            .await
            .map(tonic::Response::into_inner)
            .map_err(Into::into)
    }

    pub async fn rename_project(
        &self,
        request: pb::RenameProjectRequest,
    ) -> Result<pb::RenameProjectResponse, ClientError> {
        self.client()
            .rename_project(request)
            .await
            .map(tonic::Response::into_inner)
            .map_err(Into::into)
    }

    pub async fn resume_project(
        &self,
        request: pb::ResumeProjectRequest,
    ) -> Result<pb::ResumeProjectResponse, ClientError> {
        self.client()
            .resume_project(request)
            .await
            .map(tonic::Response::into_inner)
            .map_err(Into::into)
    }

    pub async fn update_project(
        &self,
        request: pb::UpdateProjectRequest,
    ) -> Result<pb::UpdateProjectResponse, ClientError> {
        self.client()
            .update_project(request)
            .await
            .map(tonic::Response::into_inner)
            .map_err(Into::into)
    }

    fn client(&self) -> ProjectServiceClient<AuthenticatedChannel> {
        ProjectServiceClient::with_interceptor(self.channel.clone(), self.request_policy.clone())
            .max_encoding_message_size(super::MAX_REQUEST_MESSAGE_SIZE)
            .max_decoding_message_size(super::MAX_RESPONSE_MESSAGE_SIZE)
    }
}
