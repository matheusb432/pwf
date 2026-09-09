mod blocked_by;
mod completion;
mod effort;
mod id;
pub mod order;
mod priority;
mod prompt;
mod section;
mod status;
mod tag;
mod timestamp;
mod title;

pub use blocked_by::{BlockedBy, EmptyBlockedByError};
pub use completion::{CommitRanges, TaskReport, TaskReportError};
pub use effort::{EffortTier, EffortTierError};
pub use id::{TaskId, TaskIdError};
pub use priority::{PriorityTier, PriorityTierError};
pub use prompt::TaskPrompt;
pub use section::{TaskSection, TaskSectionError};
pub use status::{ParseTaskStatusError, TaskStatus};
pub use tag::{
    EmptyTaskTagsError, InvalidTagError, ParseTaskTagsError, Tag, TagInput, TagInputError, TaskTags,
};
pub use timestamp::{TaskTimestamp, TaskTimestampError};
pub use title::{TaskTitle, TaskTitleError};

use crate::revision::ContentRevision;

/// A persisted task with parsed metadata. Launch readiness and graph validity are operation
/// concerns.
#[derive(Debug, PartialEq, Eq)]
pub struct Task {
    pub id: TaskId,
    pub title: TaskTitle,
    pub status: TaskStatus,
    pub prompt: TaskPrompt,
    pub created_at: Option<TaskTimestamp>,
    pub completed_at: Option<TaskTimestamp>,
    pub commits: Option<CommitRanges>,
    pub tags: Option<TaskTags>,
    pub effort: Option<EffortTier>,
    pub priority: Option<PriorityTier>,
    pub blocked_by: Option<BlockedBy>,
    pub section: Option<TaskSection>,
    pub revision: ContentRevision,
}
