use std::fmt;

/// Stores authored task body text without changing its formatting.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TaskBody(String);

impl TaskBody {
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }

    #[must_use]
    pub fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for TaskBody {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for TaskBody {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}
