//! Shared, compile-once regexes used across more than one engine module.
//! Each is compiled a single time per process via `LazyLock`; patterns are
//! identical to the former inline `Regex::new(...)` call sites they replaced.

use std::sync::LazyLock;

use regex::Regex;

/// `✅ YYYY-MM-DD` completion stamp; capture group 1 is the date.
/// Replaces inline sites in `clean.rs`, `done_queue.rs`, `actions/done.rs`.
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

/// A compact pending-work id: a 2–4 letter code, an optional `-`, then 1–4
/// digits. Capture 1 is the code, capture 2 the (possibly unpadded) number.
/// Matched against the already trimmed+uppercased input, so it also covers the
/// canonical `PREFIX-NNNN` shape. Replaces the inline id-shape check in
/// `domain/types.rs::canonical_pending_id`.
pub static PENDING_ID_COMPACT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([A-Z]{2,4})-?(\d{1,4})$").unwrap());
