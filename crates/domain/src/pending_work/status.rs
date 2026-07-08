use std::{fmt, str::FromStr};

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkItemStatus {
    Active,
    Done,
    Cancelled,
}

impl WorkItemStatus {
    pub const fn as_frontmatter_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Done => "done",
            Self::Cancelled => "cancelled",
        }
    }
}

impl fmt::Display for WorkItemStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_frontmatter_str())
    }
}

impl FromStr for WorkItemStatus {
    type Err = ParseWorkItemStatusError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        match raw.trim() {
            "active" => Ok(Self::Active),
            "done" => Ok(Self::Done),
            "cancelled" => Ok(Self::Cancelled),
            other => Err(ParseWorkItemStatusError {
                raw: other.to_string(),
            }),
        }
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("invalid work-item status: {raw}")]
pub struct ParseWorkItemStatusError {
    raw: String,
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::WorkItemStatus;

    #[test]
    fn parse_accepts_frontmatter_status_strings() {
        assert_eq!(
            WorkItemStatus::from_str("active").unwrap(),
            WorkItemStatus::Active
        );
        assert_eq!(
            WorkItemStatus::from_str("done").unwrap(),
            WorkItemStatus::Done
        );
        assert_eq!(
            WorkItemStatus::from_str("cancelled").unwrap(),
            WorkItemStatus::Cancelled
        );
    }

    #[test]
    fn display_roundtrips_to_frontmatter_strings() {
        assert_eq!(WorkItemStatus::Active.to_string(), "active");
        assert_eq!(WorkItemStatus::Done.to_string(), "done");
        assert_eq!(WorkItemStatus::Cancelled.to_string(), "cancelled");
    }

    #[test]
    fn parse_rejects_unknown_status() {
        assert!(WorkItemStatus::from_str("paused").is_err());
    }
}
