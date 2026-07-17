use std::{fmt, str::FromStr};

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkItemStatus {
    Active,
    Done,
    Cancelled,
}

/// Selects one lifecycle status or includes every lifecycle status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkItemStatusFilter {
    Exact(WorkItemStatus),
    All,
}

impl WorkItemStatusFilter {
    /// Returns whether the filter includes the supplied lifecycle status.
    ///
    /// # Examples
    ///
    /// ```
    /// use pwf_domain::pending_work::{WorkItemStatus, WorkItemStatusFilter};
    ///
    /// let filter = WorkItemStatusFilter::Exact(WorkItemStatus::Done);
    /// assert!(filter.includes(WorkItemStatus::Done));
    /// assert!(!filter.includes(WorkItemStatus::Active));
    /// ```
    #[must_use]
    pub fn includes(self, status: WorkItemStatus) -> bool {
        match self {
            Self::Exact(expected) => expected == status,
            Self::All => true,
        }
    }
}

impl Default for WorkItemStatusFilter {
    fn default() -> Self {
        Self::Exact(WorkItemStatus::Active)
    }
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

    use super::{WorkItemStatus, WorkItemStatusFilter};

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

    #[test]
    fn status_filter_defaults_to_active() {
        assert_eq!(
            WorkItemStatusFilter::default(),
            WorkItemStatusFilter::Exact(WorkItemStatus::Active)
        );
    }

    #[test]
    fn status_filter_matches_one_status_or_all() {
        let done = WorkItemStatusFilter::Exact(WorkItemStatus::Done);
        assert!(done.includes(WorkItemStatus::Done));
        assert!(!done.includes(WorkItemStatus::Active));

        for status in [
            WorkItemStatus::Active,
            WorkItemStatus::Done,
            WorkItemStatus::Cancelled,
        ] {
            assert!(WorkItemStatusFilter::All.includes(status));
        }
    }
}
