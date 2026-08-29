use std::{fmt, str::FromStr};

use jiff::{Timestamp, tz::TimeZone};

use crate::AppDate;

/// Stores one task instant as a canonical UTC timestamp with second precision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TaskTimestamp(Timestamp);

impl TaskTimestamp {
    /// Truncates an external instant to the task timestamp precision.
    ///
    /// # Errors
    ///
    /// Returns [`TaskTimestampError`] when the instant cannot be represented at second precision.
    pub fn from_timestamp(timestamp: Timestamp) -> Result<Self, TaskTimestampError> {
        Timestamp::from_second(timestamp.as_second())
            .map(Self)
            .map_err(|_| TaskTimestampError::new(&timestamp.to_string()))
    }

    /// Creates midnight UTC on an application date.
    pub fn at_midnight_utc(date: AppDate) -> Result<Self, TaskTimestampError> {
        format!("{date}T00:00:00Z").parse()
    }

    /// Returns the timestamp's UTC civil date.
    #[must_use]
    pub fn date(self) -> AppDate {
        AppDate::from_jiff(self.0.to_zoned(TimeZone::UTC).date())
    }
}

impl FromStr for TaskTimestamp {
    type Err = TaskTimestampError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let timestamp = raw
            .parse::<Timestamp>()
            .map_err(|_| TaskTimestampError::new(raw))?;
        let canonical = Self::from_timestamp(timestamp)?;
        if raw.len() != 20 || timestamp != canonical.0 || canonical.to_string() != raw {
            return Err(TaskTimestampError::new(raw));
        }
        Ok(canonical)
    }
}

impl TryFrom<&str> for TaskTimestamp {
    type Error = TaskTimestampError;

    fn try_from(raw: &str) -> Result<Self, Self::Error> {
        raw.parse()
    }
}

impl TryFrom<String> for TaskTimestamp {
    type Error = TaskTimestampError;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        raw.parse()
    }
}

impl fmt::Display for TaskTimestamp {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Reports a task timestamp outside the canonical UTC second-precision format.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid task timestamp {raw:?}; expected YYYY-MM-DDTHH:MM:SSZ")]
pub struct TaskTimestampError {
    raw: String,
}

impl TaskTimestampError {
    fn new(raw: &str) -> Self {
        Self {
            raw: raw.to_string(),
        }
    }
}
