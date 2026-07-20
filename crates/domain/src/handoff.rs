//! Defines local handoff values and naming policy.

use std::{fmt, str::FromStr};

/// A handoff document's lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandoffStatus {
    /// The handoff is live and belongs in the active ledger.
    Active,
    /// The linked work was completed.
    Done,
    /// The linked work was cancelled.
    Cancelled,
}

impl HandoffStatus {
    /// Returns the canonical frontmatter value.
    pub const fn as_frontmatter_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Done => "done",
            Self::Cancelled => "cancelled",
        }
    }
}

impl fmt::Display for HandoffStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_frontmatter_str())
    }
}

/// Reports an unrecognized handoff status.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid handoff status: {value}")]
pub struct ParseHandoffStatusError {
    /// Raw value rejected by the parser.
    pub value: String,
}

impl FromStr for HandoffStatus {
    type Err = ParseHandoffStatusError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "active" => Ok(Self::Active),
            "done" => Ok(Self::Done),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(ParseHandoffStatusError {
                value: value.to_string(),
            }),
        }
    }
}

/// Converts a title or caller-supplied slug to a lowercase, dash-separated file slug.
pub fn slug(value: &str) -> String {
    let mut result = String::new();
    let mut separator_pending = false;
    for character in value.trim().to_lowercase().chars() {
        if character.is_ascii_lowercase() || character.is_ascii_digit() {
            if separator_pending && !result.is_empty() {
                result.push('-');
            }
            result.push(character);
            separator_pending = false;
        } else {
            separator_pending = true;
        }
    }
    if result.is_empty() {
        "handoff".to_string()
    } else {
        result
    }
}

/// Derives the existing pending-work continuation title from a dated handoff file name.
pub fn continuation_title(file_name: &str) -> String {
    let stem = file_name.strip_suffix(".md").unwrap_or(file_name);
    let slug = strip_date_prefix(stem);
    let words = slug
        .split(['-', '_'])
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    if words.is_empty() {
        "continue handoff".to_string()
    } else {
        format!("continue {}", words.join(" "))
    }
}

fn strip_date_prefix(value: &str) -> &str {
    let bytes = value.as_bytes();
    let is_date_prefix = bytes.len() >= 11
        && bytes[0..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(u8::is_ascii_digit)
        && bytes[10] == b'-';
    if is_date_prefix { &value[11..] } else { value }
}

#[cfg(test)]
mod tests {
    use super::{HandoffStatus, continuation_title, slug};

    #[test]
    fn status_parses_only_canonical_frontmatter_values() {
        for (raw, expected) in [
            ("active", HandoffStatus::Active),
            ("done", HandoffStatus::Done),
            ("cancelled", HandoffStatus::Cancelled),
        ] {
            assert_eq!(raw.parse(), Ok(expected));
            assert_eq!(expected.to_string(), raw);
        }
        assert!("Active".parse::<HandoffStatus>().is_err());
        assert!("unknown".parse::<HandoffStatus>().is_err());
    }

    #[test]
    fn slug_preserves_the_legacy_ascii_policy() {
        assert_eq!(slug("Managed Flow"), "managed-flow");
        assert_eq!(slug("  Hello World!! "), "hello-world");
        assert_eq!(slug("déjà vu"), "d-j-vu");
        assert_eq!(slug("---"), "handoff");
    }

    #[test]
    fn continuation_title_strips_a_dated_file_prefix() {
        assert_eq!(
            continuation_title("2026-01-01-api_cleanup.md"),
            "continue api cleanup"
        );
        assert_eq!(continuation_title("2026-01-01-.md"), "continue handoff");
    }
}
