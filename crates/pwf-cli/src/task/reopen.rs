use clap::Args;
use pwf_application::task::reopen_task::{self, ReopenTask, ReopenTaskError};
use pwf_infra::obsidian::ObsidianStore;

use super::shared::{CommonArguments, Identifier};

#[derive(Args, Debug)]
pub struct Arguments {
    #[command(flatten)]
    pub(crate) identifier: Identifier,
    #[command(flatten)]
    pub(crate) common: CommonArguments,
}

use super::{render::render_reopened, shared::TaskError};

pub(super) async fn run(
    arguments: &Arguments,
    store: &ObsidianStore,
    pool: &sqlx::SqlitePool,
) -> Result<String, TaskError> {
    let id = arguments.identifier.required("reopen")?;
    let outcome = reopen_task::execute(&ReopenTask { id }, store, pool)
        .await
        .map_err(map_reopen_error)?;
    Ok(render_reopened(&outcome))
}

fn map_reopen_error(error: ReopenTaskError) -> TaskError {
    match error {
        ReopenTaskError::TaskNotFound { id } => TaskError::TaskNotFound { id },
        ReopenTaskError::UnknownProjectId {
            task_id,
            project_id,
        } => TaskError::Reopen(ReopenTaskError::UnknownProjectId {
            task_id,
            project_id,
        }),
        ReopenTaskError::WriteStore(source) => {
            TaskError::Reopen(ReopenTaskError::WriteStore(source))
        }
        ReopenTaskError::QueryProject(source) => {
            TaskError::Reopen(ReopenTaskError::QueryProject(source))
        }
    }
}
