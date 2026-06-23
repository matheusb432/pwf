use super::super::errors::PendingWorkError;
use super::resolve::resolve_id;
use crate::cli::Args;
use crate::config::Config;
use crate::engines::pending_work::run::require_id;

/// `pwf show <id>` — shorthand for `pwf resolve --show --id <id>` (PWF-0065):
/// stream the task note as Markdown (frontmatter minus exec-irrelevant keys + body)
/// for agent consumption, finding the item regardless of status.
pub(in crate::engines::pending_work) fn run_show(
    cfg: &Config,
    args: &Args,
) -> Result<String, PendingWorkError> {
    let id = require_id(args, "show")?;
    resolve_id(cfg, id, true)
}
