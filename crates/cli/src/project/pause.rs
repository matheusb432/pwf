use clap::Args;
use pwf_application::project::pause_project::{self, PauseProject};
use pwf_models::project::ProjectId;
use sqlx::SqlitePool;

use super::{output, parse_project_id};

#[derive(Args, Debug)]
pub struct Arguments {
    /// Canonical project ID.
    #[arg(value_parser = parse_project_id)]
    pub id: ProjectId,
}

pub(super) async fn run(arguments: Arguments, pool: &SqlitePool) -> Result<String, String> {
    let change = pause_project::execute(PauseProject { id: arguments.id }, pool)
        .await
        .map_err(|error| format!("project pause failed: {error}"))?;
    output::state_change(change)
}
