use std::{fmt, str::FromStr};

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Active,
    Done,
    Cancelled,
}

impl TaskStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Done => "done",
            Self::Cancelled => "cancelled",
        }
    }
}

impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for TaskStatus {
    type Err = ParseTaskStatusError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        match raw.trim() {
            "active" => Ok(Self::Active),
            "done" => Ok(Self::Done),
            "cancelled" => Ok(Self::Cancelled),
            other => Err(ParseTaskStatusError {
                raw: other.to_string(),
            }),
        }
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("invalid task status: {raw}")]
pub struct ParseTaskStatusError {
    raw: String,
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::TaskStatus;

    #[test]
    fn parse_accepts_canonical_status_values() {
        assert_eq!(TaskStatus::from_str("active").unwrap(), TaskStatus::Active);
        assert_eq!(TaskStatus::from_str("done").unwrap(), TaskStatus::Done);
        assert_eq!(
            TaskStatus::from_str("cancelled").unwrap(),
            TaskStatus::Cancelled
        );
    }

    #[test]
    fn display_roundtrips_to_canonical_status_values() {
        assert_eq!(TaskStatus::Active.to_string(), "active");
        assert_eq!(TaskStatus::Done.to_string(), "done");
        assert_eq!(TaskStatus::Cancelled.to_string(), "cancelled");
    }

    #[test]
    fn parse_rejects_unknown_status() {
        assert!(TaskStatus::from_str("paused").is_err());
    }
}
