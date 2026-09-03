mod confirmation;
mod note;
mod project;
mod session;
mod settings;
mod task;

pub(crate) use note::NoteGrpcService;
pub(crate) use project::ProjectGrpcService;
pub(crate) use session::SessionGrpcService;
pub(crate) use settings::SettingsGrpcService;
pub(crate) use task::TaskGrpcService;

pub(crate) async fn run_blocking<T>(
    operation: impl FnOnce() -> T + Send + 'static,
) -> Result<T, tonic::Status>
where
    T: Send + 'static,
{
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|error| {
            tracing::error!(error = ?error, "gRPC blocking task failed");
            tonic::Status::internal("server operation failed")
        })
}
