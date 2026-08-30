use clap::Args;
use pwf_client::{
    pb::{GetProjectRequest, ProjectStatusFilter},
    project::ProjectClient,
};
use pwf_models::project::ProjectId;

use super::{output, parse_project_id};

#[derive(Args, Debug)]
pub struct Arguments {
    /// Project ID.
    #[arg(value_parser = parse_project_id)]
    pub id: ProjectId,
}

pub(super) async fn run(arguments: Arguments, client: &ProjectClient) -> anyhow::Result<String> {
    let project = client
        .get_project(GetProjectRequest {
            id: arguments.id.to_string(),
            status: ProjectStatusFilter::IncludingPaused as i32,
        })
        .await
        .map_err(crate::rpc_error)?;
    output::render_response(project)
}
