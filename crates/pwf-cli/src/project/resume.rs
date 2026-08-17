use clap::Args;
use pwf_application::project::resume_project::{self, ResumeProjectError};
use pwf_models::project::{HomeDirectory, ProjectId};
use pwf_wire::project::{ResumeProject, ResumeProjectApiError};
use sqlx::SqlitePool;

use super::{map_task_location_error, output, parse_project_id};

#[derive(Args, Debug)]
pub struct Arguments {
    /// Project ID.
    #[arg(value_parser = parse_project_id)]
    pub id: ProjectId,
}

pub(super) async fn run(
    arguments: Arguments,
    pool: &SqlitePool,
    home: Option<HomeDirectory>,
) -> Result<String, ResumeProjectApiError> {
    let home = home.ok_or(ResumeProjectApiError::HomeDirectoryUnavailable)?;
    let change = resume_project::execute(ResumeProject { id: arguments.id }, pool, &home)
        .await
        .map_err(map_error)?;
    output::state_change(change).map_err(|error| ResumeProjectApiError::RenderJson {
        message: error.to_string(),
    })
}

fn map_error(error: ResumeProjectError) -> ResumeProjectApiError {
    match error {
        ResumeProjectError::ProjectNotFound { id } => ResumeProjectApiError::ProjectNotFound { id },
        ResumeProjectError::TaskLocation(error) => map_task_location_error(error).into(),
        ResumeProjectError::Unexpected { context, source } => ResumeProjectApiError::Unexpected {
            message: format!("{context}: {source}"),
        },
    }
}
