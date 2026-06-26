//! Shared, compile-once regexes used across more than one engine module.
//! Each is compiled a single time per process via `LazyLock`; patterns are
//! identical to the former inline `Regex::new(...)` call sites they replaced.

use std::sync::LazyLock;

use regex::Regex;

/// `✅ YYYY-MM-DD` completion stamp; capture group 1 is the date.
/// Replaces inline sites in `clean.rs`, `done_queue.rs`, `actions/check.rs`.
pub static DATE_STAMP_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"✅\s*(\d{4}-\d{2}-\d{2})").unwrap());

/// A frontmatter `status:` line (multiline). Replaces inline sites in
/// `handoff.rs` and `obsidian/note_text.rs`.
pub static STATUS_LINE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^status:.*$").unwrap());

/// A frontmatter fence line (`---`, trailing whitespace allowed), multiline.
/// Replaces inline sites in `obsidian/note_text.rs` and `actions/update.rs`.
pub static FRONTMATTER_FENCE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^---[ \t]*$").unwrap());
