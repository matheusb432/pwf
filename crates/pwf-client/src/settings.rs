use pwf_wire::pb::{self, settings_service_client::SettingsServiceClient};

use crate::{ClientError, RequestPolicy};

#[derive(Clone)]
pub struct SettingsClient {
    channel: tonic::transport::Channel,
    request_policy: RequestPolicy,
}

impl SettingsClient {
    pub(crate) fn new(channel: tonic::transport::Channel, request_policy: RequestPolicy) -> Self {
        Self {
            channel,
            request_policy,
        }
    }

    pub async fn get_user_settings(&self) -> Result<pb::GetUserSettingsResponse, ClientError> {
        SettingsServiceClient::with_interceptor(
            crate::release::ReleaseChannel(self.channel.clone()),
            self.request_policy,
        )
        .max_encoding_message_size(super::MAX_REQUEST_MESSAGE_SIZE)
        .max_decoding_message_size(super::MAX_RESPONSE_MESSAGE_SIZE)
        .get_user_settings(pb::GetUserSettingsRequest {})
        .await
        .map(tonic::Response::into_inner)
        .map_err(Into::into)
    }
}
