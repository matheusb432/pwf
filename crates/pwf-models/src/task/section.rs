use std::{fmt, str::FromStr};

/// Stores one non-empty single-line task-index section label.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TaskSection(String);

impl TaskSection {
    /// Constructs a section label after trimming outer whitespace.
    ///
    /// # Errors
    ///
    /// Returns [`TaskSectionError`] when the label is blank or contains a line break.
    pub fn try_new(raw: impl Into<String>) -> Result<Self, TaskSectionError> {
        let raw = raw.into();
        if raw.contains(['\n', '\r']) {
            return Err(TaskSectionError::Multiline);
        }
        let label = raw.trim();
        if label.is_empty() {
            return Err(TaskSectionError::Empty);
        }
        Ok(Self(label.to_string()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the case-insensitive identity used by task-list filters and groups.
    #[must_use]
    pub fn case_insensitive_key(&self) -> String {
        self.0.to_lowercase()
    }
}

impl AsRef<str> for TaskSection {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for TaskSection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for TaskSection {
    type Err = TaskSectionError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        Self::try_new(raw)
    }
}

/// Reports an invalid task-index section label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TaskSectionError {
    #[error("task section cannot be empty")]
    Empty,
    #[error("task section must be a single line")]
    Multiline,
}
