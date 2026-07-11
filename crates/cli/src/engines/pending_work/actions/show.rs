use cqrsy::Sender;
use pwf_application::{ShowPendingWorkItem, ShowPendingWorkItemHandler};
use pwf_infra::obsidian::ObsidianPendingWorkStore;

use super::super::errors::PendingWorkError;
use crate::{
    cli::Args,
    config::Config,
    engines::pending_work::{canonical_pending_id, run::require_id},
};

/// `pwf show <id>` — shorthand for `pwf resolve --show --id <id>` (PWF-0065):
/// stream the task note as Markdown (frontmatter minus exec-irrelevant keys + body)
/// for agent consumption, finding the item regardless of status.
pub(in crate::engines::pending_work) fn run_show(
    cfg: &Config,
    args: &Args,
) -> Result<String, PendingWorkError> {
    let lookup_id = require_id(args, "show")?;
    let id = match args.raw_id.as_deref() {
        Some(raw) if canonical_pending_id(raw) == lookup_id => raw,
        _ => lookup_id,
    };
    let handler = ShowPendingWorkItemHandler {
        store: ObsidianPendingWorkStore::new(cfg.clone()),
    };
    handler
        .send_now(ShowPendingWorkItem { id: id.to_string() })
        .map(pwf_application::ResolvePendingWorkOutput::into_text)
        .map_err(|error| PendingWorkError::ApplicationRead(error.to_string()))
}
