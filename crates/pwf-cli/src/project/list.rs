use clap::Args;
use pwf_client::{
    pb::{ListProjectsRequest, ProjectStatusFilter},
    project::ProjectClient,
};
use pwf_models::settings::ProjectStatusColors;

use super::output;
use crate::{
    console::Console,
    render::{render_summary, rgb_color},
    rpc_error,
};

#[derive(Args, Debug)]
pub struct Arguments {
    /// Outputs complete project records as JSON.
    #[arg(long)]
    pub json: bool,
}

pub(super) async fn run(
    arguments: Arguments,
    console: Console,
    colors: ProjectStatusColors,
    client: &ProjectClient,
) -> anyhow::Result<String> {
    let projects = client
        .list_projects(ListProjectsRequest {
            status: ProjectStatusFilter::IncludingPaused as i32,
        })
        .await
        .map_err(rpc_error)?;
    if arguments.json {
        output::projects(projects.projects)
    } else {
        Ok(projects
            .projects
            .iter()
            .map(|project| {
                let color = if project.is_paused {
                    colors.paused()
                } else {
                    colors.active()
                };
                render_summary(
                    &project.id,
                    &project.title,
                    rgb_color(color),
                    console.color(),
                )
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }
}
