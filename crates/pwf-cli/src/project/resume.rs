use clap::Args;
use pwf_client::{pb::ResumeProjectRequest, project::ProjectClient};
use pwf_models::project::ProjectId;

use super::{output, parse_project_id};

#[derive(Args, Debug)]
pub struct Arguments {
    /// Project ID.
    #[arg(value_parser = parse_project_id)]
    pub id: ProjectId,
}

pub(super) async fn run(arguments: Arguments, client: &ProjectClient) -> anyhow::Result<String> {
    let change = client
        .resume_project(ResumeProjectRequest {
            id: arguments.id.to_string(),
        })
        .await
        .map_err(crate::rpc_error)?;
    output::render_resume_project(change)
}
