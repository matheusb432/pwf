use clap::Args;
use pwf_infra::SqliteStore;

use super::output;

#[derive(Args, Debug)]
pub struct Arguments {}

pub(super) async fn run(_arguments: Arguments, database: &SqliteStore) -> Result<String, String> {
    let projects = pwf_application::project::list::execute(
        pwf_application::project::list::ListProjects {
            include_paused: true,
        },
        database,
    )
    .await
    .map_err(|error| format!("project list failed: {error}"))?;
    output::projects(projects)
}
