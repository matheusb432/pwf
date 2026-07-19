//! Defines the flattened arguments produced by clap and consumed by the engines.

use pwf_domain::pending_work::WorkItemStatusFilter;

/// Selects the color policy for `pwf session` output. [`Self::Auto`] uses TTY detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorChoice {
    #[default]
    Auto,
    Always,
    Never,
}

/// Selects the agent targeted by `pwf session` and `pwf verify`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Agent {
    #[default]
    Claude,
    Codex,
}

#[allow(
    clippy::struct_excessive_bools,
    reason = "the flattened engine DTO stores independent CLI flags"
)]
#[derive(Debug, Default, Clone)]
pub struct EngineArgs {
    pub action: Option<String>,
    pub config_path: Option<String>,
    pub notes_dir: Option<String>,
    pub repo_root: Option<String>,
    pub date: Option<String>,
    pub id: Option<String>,
    pub raw_id: Option<String>,
    pub project: Option<String>,
    pub prompt: Option<String>,
    pub prereq: Vec<String>,
    pub clear_prereq: bool,
    pub tag: Vec<String>,
    pub tags_clear: bool,
    // Shared by add, update body gating, and list filtering.
    pub effort: Option<u8>,
    pub commits: Vec<String>,
    pub review: bool,
    pub title: Option<String>,
    pub slug: Option<String>,
    pub report: Option<String>,
    // Appends Markdown verbatim without regenerating the title or Goals body.
    pub append_report: Option<String>,
    // Update and session splice these lanes into an open item's structured body.
    pub append: Option<String>,
    pub pending_work_script: Option<String>,
    // `show --path` prints the note path instead of the note markdown.
    pub path: bool,
    pub long: bool,
    pub force: bool,
    pub dry_run: bool,
    // `None` uses the default list cap; `Some(0)` disables it.
    pub number: Option<usize>,
    // The list engine resolves these raw `--order` tokens into an order specification.
    pub order: Vec<String>,
    pub status_filter: WorkItemStatusFilter,
    pub future: bool,
    pub human: bool,
    pub all: bool,
    pub words: Vec<String>,
    pub continue_handoff: bool,
    pub continue_path: Option<String>,
    pub section: Option<String>,
    pub color: ColorChoice,
    pub assume_yes: bool,
    pub inline: bool,
    pub worktree: bool,
    pub auto: bool,
    pub agent: Agent,
    // Forwarded without validation so new model names work immediately.
    pub model: Option<String>,
    pub old_code: Option<String>,
    pub new_code: Option<String>,
    pub new_path: Option<String>,
    pub manifest_path: Option<String>,
}
