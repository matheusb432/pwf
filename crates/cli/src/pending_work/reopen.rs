use clap::Args;
use pwf_application::pending_work::{
    ProjectRegistry,
    reopen_pending_work::{self, ReopenPendingWork, ReopenPendingWorkError},
};
use pwf_infra::obsidian::ObsidianStore;

use super::shared::{CommonArguments, Identifier};

#[derive(Args, Debug)]
pub struct Arguments {
    #[command(flatten)]
    pub(crate) identifier: Identifier,
    #[command(flatten)]
    pub(crate) common: CommonArguments,
}

use super::{render::render_reopened, shared::PendingWorkError};

pub(super) fn run(
    arguments: &Arguments,
    store: &ObsidianStore,
    projects: &ProjectRegistry,
) -> Result<String, PendingWorkError> {
    let id = arguments.identifier.required("reopen")?;
    let outcome = reopen_pending_work::execute(&ReopenPendingWork { id }, store, projects)
        .map_err(map_reopen_error)?;
    Ok(render_reopened(&outcome))
}

fn map_reopen_error(error: ReopenPendingWorkError) -> PendingWorkError {
    match error {
        ReopenPendingWorkError::ItemNotFound { id } => PendingWorkError::ItemNotFound { id },
        ReopenPendingWorkError::UnknownPrefix {
            pending_work_identifier,
            prefix,
        } => PendingWorkError::Reopen(ReopenPendingWorkError::UnknownPrefix {
            pending_work_identifier,
            prefix,
        }),
        ReopenPendingWorkError::WriteStore(source) => {
            PendingWorkError::Reopen(ReopenPendingWorkError::WriteStore(source))
        }
    }
}
