use prompt_lanes::{Adapter, MarkdownAdapter, parse};
use pwf_domain::pending_work::TaskTitle;

use super::text::{is_placeholder_prompt, normalize_title};

/// Upper bound (in `char`s) on an auto-inferred title. Without it, a long prompt
/// with no explicit marker becomes unreadable in the index/list surfaces.
const MAX_TITLE_CHARS: usize = 80;

/// Returns the inferred title for a pending-work prompt.
pub(super) fn inferred_title(prompt: &str) -> String {
    let title = normalize_title(&parse(prompt).capped_title(MAX_TITLE_CHARS));
    if title.is_empty() {
        TaskTitle::default().to_string()
    } else {
        title
    }
}

/// Renders the pending-work note body for a prompt.
pub(super) fn note_body(prompt: &str) -> String {
    if is_placeholder_prompt(prompt) {
        prompt.to_string()
    } else {
        MarkdownAdapter.render(&parse(prompt))
    }
}

/// Compatibility wrapper kept for older tests/callers that name the legacy
/// generated body after the `## Goals` template.
pub(super) fn goals_body(prompt: &str) -> String {
    MarkdownAdapter.render(&parse(prompt))
}
