use std::path::PathBuf;

use clap::Args;
use pwf_application::project::resume_project::{self, ResumeProject};
use pwf_infra::SqliteStore;
use pwf_models::project::ProjectPrefix;

use super::{output, parse_project_id};

#[derive(Args, Debug)]
pub struct Arguments {
    /// Canonical project ID.
    #[arg(value_parser = parse_project_id)]
    pub id: ProjectPrefix,
}

pub(super) async fn run(
    arguments: Arguments,
    database: &SqliteStore,
    home: PathBuf,
) -> Result<String, String> {
    let change = resume_project::execute(
        ResumeProject {
            id: arguments.id,
            home,
        },
        database,
    )
    .await
    .map_err(|error| format!("project resume failed: {error}"))?;
    output::state_change(change)
}
