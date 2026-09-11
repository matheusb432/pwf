use std::{fmt, str::FromStr};

use jiff::{Timestamp, fmt::temporal::Pieces, tz::Offset};

use crate::AppDate;

/// Stores one task instant and its numeric offset with second precision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TaskTimestamp {
    timestamp: Timestamp,
    offset: Offset,
}

impl TaskTimestamp {
    /// Truncates an external instant to the task timestamp precision in UTC.
    ///
    /// # Errors
    ///
    /// Returns [`TaskTimestampError`] when the instant cannot be represented at second precision.
    pub fn from_timestamp(timestamp: Timestamp) -> Result<Self, TaskTimestampError> {
        Self::from_timestamp_at_offset(timestamp, Offset::UTC)
    }

    /// Truncates an external instant and retains the supplied numeric offset.
    ///
    /// # Errors
    ///
    /// Returns [`TaskTimestampError`] when the instant cannot be represented at second precision.
    pub fn from_timestamp_at_offset(
        timestamp: Timestamp,
        offset: Offset,
    ) -> Result<Self, TaskTimestampError> {
        let timestamp = Timestamp::from_second(timestamp.as_second())
            .map_err(|_| TaskTimestampError::new(&timestamp.to_string()))?;
        Ok(Self { timestamp, offset })
    }

    /// Creates midnight UTC on an application date.
    pub fn at_midnight_utc(date: AppDate) -> Result<Self, TaskTimestampError> {
        format!("{date}T00:00:00Z").parse()
    }

    /// Returns the timestamp's civil date at its stored offset.
    #[must_use]
    pub fn date(self) -> AppDate {
        AppDate::from_jiff(self.offset.to_datetime(self.timestamp).date())
    }
}

impl FromStr for TaskTimestamp {
    type Err = TaskTimestampError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let timestamp = raw
            .parse::<Timestamp>()
            .map_err(|_| TaskTimestampError::new(raw))?;
        let offset = Pieces::parse(raw)
            .ok()
            .and_then(|pieces| pieces.to_numeric_offset())
            .ok_or_else(|| TaskTimestampError::new(raw))?;
        let canonical = Self::from_timestamp_at_offset(timestamp, offset)?;
        let is_canonical_numeric_offset = canonical.to_string() == raw;
        let is_canonical_utc = raw.len() == 20 && timestamp.to_string() == raw;
        if !is_canonical_numeric_offset && !is_canonical_utc {
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
        Pieces::from((self.timestamp, self.offset)).fmt(formatter)
    }
}

/// Reports a task timestamp outside the canonical RFC 3339 second-precision format.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "invalid task timestamp {raw:?}; expected YYYY-MM-DDTHH:MM:SS with Z or a +HH:MM/-HH:MM offset"
)]
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
