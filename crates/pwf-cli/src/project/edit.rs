use clap::Args;
use pwf_client::{
    pb::{StringFieldUpdate, UpdateProjectRequest, string_field_update},
    project::ProjectClient,
};
use pwf_models::project::{ProjectId, ProjectSourceValue};

use super::{parse_project_id, parse_project_source};

#[derive(Args, Debug)]
pub struct Arguments {
    /// Project ID.
    #[arg(value_parser = parse_project_id)]
    pub id: ProjectId,
    /// Replacement project source directory.
    #[arg(long, value_parser = parse_project_source)]
    pub source: ProjectSourceValue,
}

pub(super) async fn run(arguments: Arguments, client: &ProjectClient) -> anyhow::Result<String> {
    client
        .update_project(UpdateProjectRequest {
            id: arguments.id.to_string(),
            source_value: Some(StringFieldUpdate {
                operation: Some(string_field_update::Operation::Update(
                    arguments.source.to_string(),
                )),
            }),
        })
        .await
        .map_err(crate::rpc_error)?;
    Ok(String::new())
}
