use clap::Args;
use pwf_application::project::list_projects::{self, ListProjectsError};
use pwf_wire::project::{ListProjects, ListProjectsApiError, ProjectStatusFilter};
use sqlx::SqlitePool;

use super::output;

#[derive(Args, Debug)]
pub struct Arguments {}

pub(super) async fn run(
    _arguments: Arguments,
    pool: &SqlitePool,
) -> Result<String, ListProjectsApiError> {
    let projects = list_projects::execute(
        ListProjects {
            status: ProjectStatusFilter::IncludingPaused,
        },
        pool,
    )
    .await
    .map_err(map_error)?;
    output::projects(projects).map_err(|error| ListProjectsApiError::RenderJson {
        message: error.to_string(),
    })
}

fn map_error(error: ListProjectsError) -> ListProjectsApiError {
    match error {
        ListProjectsError::Unexpected { context, source } => ListProjectsApiError::Unexpected {
            message: format!("{context}: {source}"),
        },
    }
}
