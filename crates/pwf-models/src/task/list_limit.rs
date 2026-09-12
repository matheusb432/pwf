/// Caps one task-list response before transport message limits apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskListLimit(usize);

impl TaskListLimit {
    pub const MAX: usize = 100_000;

    /// Constructs a nonzero task-list limit within the supported response cap.
    ///
    /// # Errors
    ///
    /// Returns [`TaskListLimitError`] when `value` is zero or exceeds [`Self::MAX`].
    pub fn try_new(value: usize) -> Result<Self, TaskListLimitError> {
        if (1..=Self::MAX).contains(&value) {
            Ok(Self(value))
        } else {
            Err(TaskListLimitError { value })
        }
    }

    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

/// Reports a task-list limit outside the supported response cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{value} must be between 1 and {}", TaskListLimit::MAX)]
pub struct TaskListLimitError {
    value: usize,
}

impl Default for TaskListLimit {
    fn default() -> Self {
        Self(10)
    }
}
