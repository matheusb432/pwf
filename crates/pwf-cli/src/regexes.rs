//! Defines shared regexes compiled once per process.

use std::sync::LazyLock;

use regex::Regex;

/// Matches a `✅ YYYY-MM-DD` completion stamp and captures the date in group 1.
pub static DATE_STAMP_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"✅\s*(\d{4}-\d{2}-\d{2})").unwrap());

/// Matches a frontmatter `status:` line in multiline input.
pub static STATUS_LINE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^status:.*$").unwrap());

/// Matches a frontmatter fence with optional trailing whitespace in multiline input.
pub static FRONTMATTER_FENCE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^---[ \t]*$").unwrap());

/// Matches an uppercase compact task ID with a 2-4 letter code and 1-4 digits.
/// Group 1 captures the code, and group 2 captures the possibly unpadded number.
pub static TASK_ID_COMPACT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([A-Z]{2,4})-?(\d{1,4})$").unwrap());
