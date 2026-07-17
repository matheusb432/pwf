use pwf_application::{AppDbStore, NoteMarkdownSource, PendingWorkItem};

use super::{super::errors::PendingWorkError, resolve::resolve_output};
use crate::{cli::EngineArgs, config::Config};

/// Returns the complete Markdown for an item regardless of status.
pub(in crate::engines::pending_work) fn run_show<S>(
    cfg: &Config,
    store: &S,
    args: &EngineArgs,
) -> Result<String, PendingWorkError>
where
    S: AppDbStore<PendingWorkItem> + NoteMarkdownSource,
{
    resolve_output(cfg, store, args, "show", true)
}
