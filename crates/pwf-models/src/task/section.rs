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
    pub fn human() -> Self {
        Self("Human".to_string())
    }

    #[must_use]
    pub fn future() -> Self {
        Self("Future".to_string())
    }

    #[must_use]
    pub fn low_priority() -> Self {
        Self("Low-prio".to_string())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
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

/// Selects the index placement for a newly created task.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum IndexSection {
    #[default]
    General,
    Human,
}

impl IndexSection {
    #[must_use]
    pub fn task_section(self) -> Option<TaskSection> {
        match self {
            Self::General => None,
            Self::Human => Some(TaskSection::human()),
        }
    }
}
