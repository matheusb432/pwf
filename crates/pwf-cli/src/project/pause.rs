use clap::Args;
use pwf_application::project::pause_project::{self, PauseProjectError};
use pwf_models::project::ProjectId;
use pwf_wire::project::{PauseProject, PauseProjectApiError};
use sqlx::SqlitePool;

use super::{output, parse_project_id};

#[derive(Args, Debug)]
pub struct Arguments {
    /// Project ID.
    #[arg(value_parser = parse_project_id)]
    pub id: ProjectId,
}

pub(super) async fn run(
    arguments: Arguments,
    pool: &SqlitePool,
) -> Result<String, PauseProjectApiError> {
    let change = pause_project::execute(PauseProject { id: arguments.id }, pool)
        .await
        .map_err(map_error)?;
    output::state_change(change).map_err(|error| PauseProjectApiError::RenderJson {
        message: error.to_string(),
    })
}

fn map_error(error: PauseProjectError) -> PauseProjectApiError {
    match error {
        PauseProjectError::ProjectNotFound { id } => PauseProjectApiError::ProjectNotFound { id },
        PauseProjectError::Unexpected { context, source } => PauseProjectApiError::Unexpected {
            message: format!("{context}: {source}"),
        },
    }
}
