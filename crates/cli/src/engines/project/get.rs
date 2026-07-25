use clap::Args;
use pwf_domain::project::ProjectPrefix;
use pwf_infra::SqliteStore;

use super::{output, parse_project_id};

#[derive(Args, Debug)]
pub struct Arguments {
    /// Canonical project ID.
    #[arg(value_parser = parse_project_id)]
    pub id: ProjectPrefix,
}

pub(super) async fn run(arguments: Arguments, database: &SqliteStore) -> Result<String, String> {
    let project = pwf_application::project::get::execute(
        pwf_application::project::get::GetProject { id: arguments.id },
        database,
    )
    .await
    .map_err(|error| format!("project get failed: {error}"))?;
    output::project(project)
}
