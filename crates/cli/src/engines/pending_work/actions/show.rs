use pwf_application::{AppDbStore, NoteMarkdownSource, PendingWorkItem};

use super::{super::errors::PendingWorkError, resolve::resolve_output};
use crate::{cli::Args, config::Config};

/// `pwf show <id>` — shorthand for `pwf resolve --show --id <id>` (PWF-0065):
/// stream the task note as Markdown (frontmatter + body) for agent consumption,
/// finding the item regardless of status.
pub(in crate::engines::pending_work) fn run_show<S>(
    cfg: &Config,
    store: &S,
    args: &Args,
) -> Result<String, PendingWorkError>
where
    S: AppDbStore<PendingWorkItem> + NoteMarkdownSource,
{
    resolve_output(cfg, store, args, "show", true)
}
