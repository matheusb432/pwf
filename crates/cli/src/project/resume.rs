use std::path::PathBuf;

use clap::Args;
use pwf_application::project::resume_project::{self, ResumeProject};
use pwf_models::project::ProjectId;
use sqlx::SqlitePool;

use super::{output, parse_project_id};

#[derive(Args, Debug)]
pub struct Arguments {
    /// Canonical project ID.
    #[arg(value_parser = parse_project_id)]
    pub id: ProjectId,
}

pub(super) async fn run(
    arguments: Arguments,
    pool: &SqlitePool,
    home: PathBuf,
) -> Result<String, String> {
    let change = resume_project::execute(
        ResumeProject {
            id: arguments.id,
            home,
        },
        pool,
    )
    .await
    .map_err(|error| format!("project resume failed: {error}"))?;
    output::state_change(change)
}
