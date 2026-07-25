use clap::Args;
use pwf_application::project::list_projects::{self, ListProjects};
use pwf_infra::SqliteStore;

use super::output;

#[derive(Args, Debug)]
pub struct Arguments {}

pub(super) async fn run(_arguments: Arguments, database: &SqliteStore) -> Result<String, String> {
    let projects = list_projects::execute(
        ListProjects {
            include_paused: true,
        },
        database,
    )
    .await
    .map_err(|error| format!("project list failed: {error}"))?;
    output::projects(projects)
}
