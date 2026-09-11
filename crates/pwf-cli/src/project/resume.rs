use clap::Args;
use pwf_client::{pb::ResumeProjectRequest, project::ProjectClient};
use pwf_models::project::ProjectId;

use super::{output, parse_project_id};

#[derive(Args, Debug)]
pub struct Arguments {
    /// Outputs the result as JSON.
    #[arg(long)]
    pub json: bool,
    /// Project ID.
    #[arg(value_parser = parse_project_id)]
    pub id: ProjectId,
}

pub(super) async fn run(
    arguments: Arguments,
    console: crate::console::Console,
    colors: pwf_models::settings::ProjectStatusColors,
    client: &ProjectClient,
) -> anyhow::Result<String> {
    let change = client
        .resume_project(ResumeProjectRequest {
            id: arguments.id.to_string(),
        })
        .await
        .map_err(crate::rpc_error)?;
    if arguments.json {
        output::render_resume_project(change)
    } else {
        let action = if change.changed {
            output::ProjectMutationAction::Resumed
        } else {
            output::ProjectMutationAction::AlreadyActive
        };
        let project = change.project.ok_or_else(|| {
            anyhow::anyhow!("pwf-server returned a project state change without a project")
        })?;
        Ok(output::render_mutation(
            action,
            project,
            colors,
            console.color(),
        ))
    }
}
