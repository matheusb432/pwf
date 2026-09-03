use pwf_application::{
    ports::user_settings::UserSettingsLoadError,
    settings::get_user_settings::{self, GetUserSettings, GetUserSettingsError},
};
use pwf_wire::{
    pb::{self, settings_service_server::SettingsService},
    proto,
};
use tonic::{Request, Response, Status};

use super::run_blocking;
use crate::AppState;

pub(crate) struct SettingsGrpcService {
    state: AppState,
}

impl SettingsGrpcService {
    pub(crate) fn new(state: AppState) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl SettingsService for SettingsGrpcService {
    async fn get_user_settings(
        &self,
        _request: Request<pb::GetUserSettingsRequest>,
    ) -> Result<Response<pb::GetUserSettingsResponse>, Status> {
        let store = self.state.user_settings.clone();
        run_blocking(move || get_user_settings::execute(GetUserSettings, &store))
            .await?
            .map(proto::settings::get_user_settings_response)
            .map(Response::new)
            .map_err(get_user_settings_status)
    }
}

fn get_user_settings_status(error: GetUserSettingsError) -> Status {
    match error {
        GetUserSettingsError::Settings(UserSettingsLoadError::InvalidConfiguration(error)) => {
            Status::failed_precondition(error.to_string())
        }
        GetUserSettingsError::Settings(UserSettingsLoadError::Adapter(error)) => {
            Status::internal(error.to_string())
        }
    }
}
