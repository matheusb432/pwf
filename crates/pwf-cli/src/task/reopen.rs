use clap::Args;
use pwf_application::task::reopen_task::{self, ReopenTask};
use pwf_infra::obsidian::ObsidianStore;

use super::shared::Identifier;

#[derive(Args, Debug)]
pub struct Arguments {
    #[command(flatten)]
    pub(crate) identifier: Identifier,
}

use super::{render::render_reopened, shared::TaskError};

pub(super) async fn run(
    arguments: &Arguments,
    store: &ObsidianStore,
    pool: &sqlx::SqlitePool,
) -> Result<String, TaskError> {
    let id = arguments.identifier.required("reopen")?;
    let outcome = reopen_task::execute(&ReopenTask { id }, store, pool).await?;
    Ok(render_reopened(&outcome))
}
