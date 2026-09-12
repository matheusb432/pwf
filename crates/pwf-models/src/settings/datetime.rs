use std::str::FromStr;

use crate::task::TaskTimestamp;

const DEFAULT_FORMAT: &str = "%d/%m/%Y %H:%M";

/// A strftime pattern checked against a complete timestamp with a fixed offset.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DateTimeFormat(String);

impl DateTimeFormat {
    pub fn format(&self, timestamp: TaskTimestamp) -> Result<String, jiff::Error> {
        timestamp.format(&self.0)
    }
}

impl AsRef<str> for DateTimeFormat {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl FromStr for DateTimeFormat {
    type Err = DateTimeFormatError;

    fn from_str(pattern: &str) -> Result<Self, Self::Err> {
        Self::try_from(pattern.to_string())
    }
}

impl TryFrom<String> for DateTimeFormat {
    type Error = DateTimeFormatError;

    fn try_from(pattern: String) -> Result<Self, Self::Error> {
        if pattern.trim().is_empty() {
            return Err(DateTimeFormatError::Empty);
        }
        jiff::fmt::strtime::format(
            &pattern,
            &jiff::Timestamp::UNIX_EPOCH.to_zoned(jiff::tz::Offset::UTC.to_time_zone()),
        )
        .map_err(DateTimeFormatError::Pattern)?;
        Ok(Self(pattern))
    }
}

impl Default for DateTimeFormat {
    fn default() -> Self {
        Self(DEFAULT_FORMAT.to_string())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DateTimeFormatError {
    #[error("datetime_format must not be empty")]
    Empty,
    #[error("invalid datetime_format: {0}")]
    Pattern(#[source] jiff::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn datetime_format_preserves_the_stored_clock_and_offset() {
        let timestamp = "2026-09-12T01:38:00-03:00".parse().unwrap();
        assert_eq!(
            DateTimeFormat::default().format(timestamp).unwrap(),
            "12/09/2026 01:38"
        );
        let custom: DateTimeFormat = "%Y-%m-%d %H:%M %:z".parse().unwrap();
        assert_eq!(custom.format(timestamp).unwrap(), "2026-09-12 01:38 -03:00");
    }

    #[test]
    fn datetime_format_rejects_invalid_patterns_at_construction() {
        for pattern in ["", "  ", "%", "%J"] {
            assert!(pattern.parse::<DateTimeFormat>().is_err(), "{pattern}");
        }
    }
}
