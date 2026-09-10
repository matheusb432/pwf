use pwf_client::{
    pb::{ProjectStatusFilter, ResolveProjectRequest},
    project::ProjectClient,
};
use pwf_models::project::{ProjectId, ProjectSelector};

pub(crate) async fn resolve_project_id(
    selector: &ProjectSelector,
    client: &ProjectClient,
) -> anyhow::Result<ProjectId> {
    let response = client
        .resolve_project(ResolveProjectRequest {
            selector: selector.to_string(),
            status: ProjectStatusFilter::ActiveOnly as i32,
        })
        .await
        .map_err(crate::rpc_error)?;
    ProjectId::try_new(response.id)
        .map_err(|error| anyhow::anyhow!("pwf-server returned an invalid project ID: {error}"))
}
