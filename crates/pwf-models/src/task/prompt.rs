use std::fmt;

/// Stores authored task prompt text without changing its formatting.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TaskPrompt(String);

impl TaskPrompt {
    #[must_use]
    pub fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for TaskPrompt {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for TaskPrompt {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}
