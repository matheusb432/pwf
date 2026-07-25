//! Defines local handoff values.

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

#[cfg(test)]
mod tests {
    use super::HandoffStatus;

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
}
