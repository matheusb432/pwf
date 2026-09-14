mod blocked_by;
mod body;
mod completion;
mod effort;
mod id;
mod list_limit;
pub mod order;
mod priority;
mod status;
mod tag;
mod timestamp;
mod title;

pub use blocked_by::{BlockedBy, EmptyBlockedByError};
pub use body::TaskBody;
pub use completion::{CommitRanges, TaskReport, TaskReportError};
pub use effort::{EffortTier, EffortTierError};
pub use id::{TaskId, TaskIdError};
pub use list_limit::{TaskListLimit, TaskListLimitError};
pub use priority::{PriorityTier, PriorityTierError};
pub use status::{ParseTaskStatusError, TaskStatus};
pub use tag::{
    EmptyTaskTagsError, InvalidTagError, ParseTaskTagsError, Tag, TagInput, TagInputError, TaskTags,
};
pub use timestamp::{TaskTimestamp, TaskTimestampError};
pub use title::{TaskTitle, TaskTitleError};

use crate::revision::ContentRevision;

#[derive(Debug, PartialEq, Eq)]
pub struct Task {
    pub id: TaskId,
    pub title: TaskTitle,
    pub status: TaskStatus,
    pub body: TaskBody,
    pub created_at: Option<TaskTimestamp>,
    pub completed_at: Option<TaskTimestamp>,
    pub commits: Option<CommitRanges>,
    pub tags: Option<TaskTags>,
    pub effort: Option<EffortTier>,
    pub priority: Option<PriorityTier>,
    pub blocked_by: Option<BlockedBy>,
    pub revision: ContentRevision,
}
