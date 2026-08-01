use clap::Args;
use pwf_application::project::list_projects::{self, ListProjects};
use sqlx::SqlitePool;

use super::output;

#[derive(Args, Debug)]
pub struct Arguments {}

pub(super) async fn run(_arguments: Arguments, pool: &SqlitePool) -> Result<String, String> {
    let projects = list_projects::execute(
        ListProjects {
            include_paused: true,
        },
        pool,
    )
    .await
    .map_err(|error| format!("project list failed: {error}"))?;
    output::projects(projects)
}
