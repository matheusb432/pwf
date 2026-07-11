use cqrsy::Sender;
use pwf_application::{ResolvePendingWorkItem, ResolvePendingWorkItemHandler};
use pwf_infra::obsidian::ObsidianPendingWorkStore;

use super::super::errors::PendingWorkError;
use crate::{
    cli::Args,
    config::Config,
    engines::pending_work::{canonical_pending_id, run::require_id},
};

pub(in crate::engines::pending_work) fn run_resolve(
    cfg: &Config,
    args: &Args,
) -> Result<String, PendingWorkError> {
    let lookup_id = require_id(args, "resolve")?;
    let id = match args.raw_id.as_deref() {
        Some(raw) if canonical_pending_id(raw) == lookup_id => raw,
        _ => lookup_id,
    };
    let handler = ResolvePendingWorkItemHandler {
        store: ObsidianPendingWorkStore::new(cfg.clone()),
    };
    handler
        .send_now(ResolvePendingWorkItem {
            id: id.to_string(),
            show: args.show,
        })
        .map(pwf_application::ResolvePendingWorkOutput::into_text)
        .map_err(|error| PendingWorkError::ApplicationRead(error.to_string()))
}
