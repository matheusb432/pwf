use std::{fmt, str::FromStr};

use jiff::civil::Date;

/// Stores one canonical application-local civil date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AppDate(Date);

impl AppDate {
    /// Creates an application-local civil date from Gregorian calendar components.
    ///
    /// # Errors
    ///
    /// Returns [`AppDateError`] when the components do not form a canonical
    /// `YYYY-MM-DD` date.
    pub fn from_calendar_date(year: i16, month: i8, day: i8) -> Result<Self, AppDateError> {
        let raw = format!("{year:04}-{month:02}-{day:02}");
        raw.parse()
    }

    pub(crate) const fn from_jiff(date: Date) -> Self {
        Self(date)
    }
}

impl FromStr for AppDate {
    type Err = AppDateError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let date = raw.parse::<Date>().map_err(|_| AppDateError::new(raw))?;
        if raw.len() != 10 || date.to_string() != raw {
            return Err(AppDateError::new(raw));
        }
        Ok(Self(date))
    }
}

impl TryFrom<&str> for AppDate {
    type Error = AppDateError;

    fn try_from(raw: &str) -> Result<Self, Self::Error> {
        raw.parse()
    }
}

impl TryFrom<String> for AppDate {
    type Error = AppDateError;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        raw.parse()
    }
}

impl fmt::Display for AppDate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Reports a date that is invalid or is not encoded as `YYYY-MM-DD`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid application date {raw:?}; expected YYYY-MM-DD")]
pub struct AppDateError {
    raw: String,
}

impl AppDateError {
    fn new(raw: &str) -> Self {
        Self {
            raw: raw.to_string(),
        }
    }
}
