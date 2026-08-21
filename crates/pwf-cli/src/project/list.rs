use clap::Args;
use pwf_client::{
    project::ProjectClient,
    v1::{ListProjectsRequest, ProjectStatusFilter},
};

use super::output;

#[derive(Args, Debug)]
pub struct Arguments {}

pub(super) async fn run(_arguments: Arguments, client: &ProjectClient) -> anyhow::Result<String> {
    let projects = client
        .list_projects(ListProjectsRequest {
            status: ProjectStatusFilter::IncludingPaused as i32,
        })
        .await
        .map_err(crate::rpc_error)?;
    output::projects(projects.projects)
}
