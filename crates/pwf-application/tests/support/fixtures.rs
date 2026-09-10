use pwf_models::{
    revision::ContentRevision,
    task::{BlockedBy, TaskId, TaskStatus, TaskTimestamp},
};
use pwf_wire::task::{Materialization, StoredBlockedBy, TaskRecord};

pub(crate) fn task_timestamp(raw: impl AsRef<str>) -> TaskTimestamp {
    raw.as_ref().parse().unwrap()
}

pub(crate) fn task_record(id: &str) -> TaskRecord {
    TaskRecord {
        id: TaskId::try_new(id).unwrap(),
        title: "tray gui".to_string(),
        status: TaskStatus::Active,
        created_at: Some(task_timestamp("2026-01-01T00:00:00Z")),
        completed_at: None,
        commits: None,
        tags: None,
        effort: None,
        priority: None,
        blocked_by: pwf_wire::task::StoredBlockedBy::Absent,
        section: None,
        body: "\nbody\n".to_string(),
        source: "body".to_string(),
        locator: pwf_wire::task::TaskNotePath::new(format!("/mem/foo-bar/{id}.md").into()),
        placement: None,
        materialization: Materialization::NoteFile,
        revision: ContentRevision::try_new("0".repeat(64)).unwrap(),
    }
}

pub(crate) fn blocked_by(ids: &[&str]) -> BlockedBy {
    BlockedBy::try_new(ids.iter().map(|id| id.parse().unwrap())).unwrap()
}

pub(crate) fn stored_blocked_by(ids: &[&str]) -> StoredBlockedBy {
    StoredBlockedBy::Valid(blocked_by(ids))
}
