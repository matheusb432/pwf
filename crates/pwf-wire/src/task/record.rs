use std::{fmt, num::NonZeroUsize};

use pwf_models::{
    revision::ContentRevision,
    task::{
        BlockedBy, CommitRanges, EffortTier, PriorityTier, Task, TaskId, TaskPrompt, TaskSection,
        TaskStatus, TaskTags, TaskTimestamp, TaskTitle,
    },
};

use super::{TaskIndexPath, TaskNotePath};

/// Represents the optional `blocked_by` property after infrastructure parsing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum StoredBlockedBy {
    #[default]
    Absent,
    Valid(BlockedBy),
    Malformed {
        raw: String,
        reason: String,
    },
}

impl StoredBlockedBy {
    #[must_use]
    pub fn valid(&self) -> Option<&BlockedBy> {
        match self {
            Self::Valid(blocked_by) => Some(blocked_by),
            Self::Absent | Self::Malformed { .. } => None,
        }
    }
}

/// Locates a record's open link by index display path and one-based line number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexPlacement {
    pub index_path: TaskIndexPath,
    pub line: NonZeroUsize,
}

/// How the vault materializes a record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Materialization {
    NoteFile,
    /// An index link without a note file; `expected` is the platform-formatted diagnostic path.
    MissingNote {
        expected: TaskNotePath,
    },
}

/// Carries a task record with raw metadata and its backing revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRecord {
    pub id: TaskId,
    pub title: String,
    pub status: TaskStatus,
    pub created_at: Option<TaskTimestamp>,
    pub completed_at: Option<TaskTimestamp>,
    pub commits: Option<String>,
    /// Preserves raw `tags:` frontmatter for lazy validation by tag-filtered reads.
    pub tags: Option<RawTaskTags>,
    pub effort: Option<String>,
    pub priority: Option<String>,
    /// Carries typed or malformed `blocked_by` metadata for boundary-specific handling.
    pub blocked_by: StoredBlockedBy,
    pub section: Option<TaskSection>,
    /// Preserves the note body below frontmatter verbatim.
    pub body: String,
    /// Preserves the raw note source byte-for-byte for `pwf task get`.
    pub source: String,
    /// Stores the display path; writes relocate the record by scope and id.
    pub locator: TaskNotePath,
    /// Identifies the open index link used to read this record, when present.
    pub placement: Option<IndexPlacement>,
    pub materialization: Materialization,
    /// Opaque revision of the exact persisted file bytes backing this record.
    pub revision: ContentRevision,
}

/// Preserves authored task-tags frontmatter until a use case requires validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawTaskTags(String);

impl RawTaskTags {
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }

    #[must_use]
    pub fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }
}

impl AsRef<str> for RawTaskTags {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RawTaskTags {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TaskRecordError {
    #[error("task {id} has no note at {path}")]
    MissingNote { id: TaskId, path: TaskNotePath },
    #[error("Invalid task {field} for {id} at {path}: {source}")]
    Metadata {
        id: TaskId,
        path: TaskNotePath,
        field: &'static str,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
    #[error("task {id} at {path} has malformed blocked_by metadata {raw:?}: {reason}")]
    BlockedBy {
        id: TaskId,
        path: TaskNotePath,
        raw: Box<str>,
        reason: Box<str>,
    },
}

impl TaskRecord {
    pub fn into_task(self) -> Result<Task, TaskRecordError> {
        if let Materialization::MissingNote { expected } = self.materialization {
            return Err(TaskRecordError::MissingNote {
                id: self.id,
                path: expected,
            });
        }
        let invalid =
            |field, source: Box<dyn std::error::Error + Send + Sync>| TaskRecordError::Metadata {
                id: self.id.clone(),
                path: self.locator.clone(),
                field,
                source,
            };
        let title =
            TaskTitle::try_new(self.title).map_err(|error| invalid("title", Box::new(error)))?;
        let tags = self
            .tags
            .as_ref()
            .map(|raw| TaskTags::parse_frontmatter(raw.as_ref()))
            .transpose()
            .map_err(|error| invalid("tags", Box::new(error)))?;
        let effort = self
            .effort
            .as_deref()
            .map(|raw| raw.trim().parse::<EffortTier>())
            .transpose()
            .map_err(|error| invalid("effort", Box::new(error)))?;
        let priority = self
            .priority
            .as_deref()
            .map(|raw| raw.trim().parse::<PriorityTier>())
            .transpose()
            .map_err(|error| invalid("priority", Box::new(error)))?;
        let commits = self
            .commits
            .map(|raw| CommitRanges::try_new(unquote_scalar(raw)))
            .transpose()
            .map_err(|error| invalid("commits", Box::new(error)))?;
        let blocked_by = match self.blocked_by {
            StoredBlockedBy::Absent => None,
            StoredBlockedBy::Valid(value) => Some(value),
            StoredBlockedBy::Malformed { raw, reason } => {
                return Err(TaskRecordError::BlockedBy {
                    id: self.id,
                    path: self.locator,
                    raw: raw.into_boxed_str(),
                    reason: reason.into_boxed_str(),
                });
            }
        };
        Ok(Task {
            id: self.id,
            title,
            status: self.status,
            prompt: TaskPrompt::new(self.body),
            created_at: self.created_at,
            completed_at: self.completed_at,
            commits,
            tags,
            effort,
            priority,
            blocked_by,
            section: self.section,
            revision: self.revision,
        })
    }
}

fn unquote_scalar(mut raw: String) -> String {
    let unquoted = raw
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            raw.strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(&raw);
    if unquoted.len() != raw.len() {
        raw.remove(0);
        raw.pop();
    }
    raw
}
