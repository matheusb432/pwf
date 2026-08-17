use clap::Args;
use pwf_application::project::get_project::{self, GetProjectError};
use pwf_models::project::ProjectId;
use pwf_wire::project::{GetProject, GetProjectApiError, ProjectStatusFilter};
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
) -> Result<String, GetProjectApiError> {
    let project = get_project::execute(
        GetProject {
            id: arguments.id,
            status: ProjectStatusFilter::IncludingPaused,
        },
        pool,
    )
    .await
    .map_err(map_error)?;
    output::project(project).map_err(|error| GetProjectApiError::RenderJson {
        message: error.to_string(),
    })
}

fn map_error(error: GetProjectError) -> GetProjectApiError {
    match error {
        GetProjectError::ProjectNotFound { id } => GetProjectApiError::ProjectNotFound { id },
        GetProjectError::Unexpected { context, source } => GetProjectApiError::Unexpected {
            message: format!("{context}: {source}"),
        },
    }
}
