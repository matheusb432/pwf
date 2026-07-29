use clap::Args;
use pwf_application::project::pause_project::{self, PauseProject};
use pwf_infra::SqliteStore;
use pwf_models::project::ProjectPrefix;

use super::{output, parse_project_id};

#[derive(Args, Debug)]
pub struct Arguments {
    /// Canonical project ID.
    #[arg(value_parser = parse_project_id)]
    pub id: ProjectPrefix,
}

pub(super) async fn run(arguments: Arguments, database: &SqliteStore) -> Result<String, String> {
    let change = pause_project::execute(PauseProject { id: arguments.id }, database)
        .await
        .map_err(|error| format!("project pause failed: {error}"))?;
    output::state_change(change)
}
