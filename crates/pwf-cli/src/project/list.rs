use clap::Args;
use pwf_application::project::list_projects::{self, ListProjects};
use pwf_wire::project::ProjectStatusFilter;
use sqlx::SqlitePool;

use super::output;

#[derive(Args, Debug)]
pub struct Arguments {}

pub(super) async fn run(_arguments: Arguments, pool: &SqlitePool) -> Result<String, String> {
    let projects = list_projects::execute(
        ListProjects {
            status: ProjectStatusFilter::ALL,
        },
        pool,
    )
    .await
    .map_err(|error| format!("project list failed: {error}"))?;
    output::projects(projects)
}
