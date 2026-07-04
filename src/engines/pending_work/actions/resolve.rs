use super::super::{
    errors::PendingWorkError, obsidian::store::ObsidianStore, query::find_pending_item,
};
use crate::{
    cli::Args,
    config::Config,
    engines::pending_work::{
        Item, errors, naming::path_str, query::find_item_note_file, run::require_id,
    },
};

/// Frontmatter keys irrelevant to *executing* a task — dropped by `resolve --show`.
const SHOW_FRONTMATTER_DENYLIST: &[&str] = &["created"];

pub(in crate::engines::pending_work) fn run_resolve(
    cfg: &Config,
    args: &Args,
) -> Result<String, PendingWorkError> {
    let id = require_id(args, "resolve")?;
    resolve_id(cfg, id, args.show)
}

/// Status-agnostic resolution shared by `resolve` and its `show` alias: locate the
/// item by id — active index item, freshly-closed note still in the project dir, or
/// one evicted to `_archive` — then render its path or (with `show`) its markdown.
pub(super) fn resolve_id(cfg: &Config, id: &str, show: bool) -> Result<String, PendingWorkError> {
    match find_pending_item(cfg, id) {
        Ok(item) => resolve_active_item(&item, show),
        // ? Fallback to item on project dir `_archive`
        Err(errors::PendingWorkError::ItemNotFound { id }) => match find_item_note_file(cfg, &id) {
            Some(file) => resolve_file(&file, show),
            None => Err(errors::PendingWorkError::ItemNotFound { id }),
        },
        Err(other) => Err(other),
    }
}

/// Render an active index item for `resolve`: the note path, or with `show` the
/// note markdown (file-model) / parsed body (legacy inline).
fn resolve_active_item(item: &Item, show: bool) -> Result<String, errors::PendingWorkError> {
    if show {
        // File-model: stream the note as markdown, minus exec-irrelevant frontmatter.
        // Legacy inline items have no standalone file → emit the parsed body.
        return match item.file_path.as_deref() {
            Some(file) => resolve_file(std::path::Path::new(file), true),
            None => Ok(item.prompt.clone()),
        };
    }
    Ok(item.file_path.as_deref().unwrap_or(&item.note).to_string())
}

/// Render a per-item note file for `resolve`: its forward-slash path, or with
/// `show` the note markdown minus exec-irrelevant frontmatter.
fn resolve_file(file: &std::path::Path, show: bool) -> Result<String, errors::PendingWorkError> {
    if show {
        let raw = ObsidianStore::read_item_file(file)?;
        return Ok(crate::frontmatter::strip_frontmatter_keys(
            &raw,
            SHOW_FRONTMATTER_DENYLIST,
        ));
    }
    Ok(path_str(file))
}
