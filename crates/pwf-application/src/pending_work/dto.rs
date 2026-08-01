use pwf_models::{
    pending_work::{WorkItemId, WorkItemStatus},
    project::Project,
};

use crate::ports::pending_work_record::ItemPatch;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrerequisiteStatus {
    pub id: WorkItemId,
    pub status: Option<WorkItemStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingWorkItemView {
    pub id: String,
    pub project: String,
    pub status: WorkItemStatus,
    pub session: String,
    pub prompt: String,
    pub repo: Option<String>,
    pub note: String,
    pub item_file: Option<String>,
    pub line: usize,
    pub format: String,
    pub launchable: bool,
    pub needs_prompt: bool,
    pub issues: Vec<String>,
    pub section: Option<String>,
    pub prereq: Option<String>,
    pub prerequisite_statuses: Vec<PrerequisiteStatus>,
    pub effort: Option<String>,
    pub tags: Option<String>,
    pub created: Option<String>,
}

pub(in crate::pending_work) struct PendingWorkItemIdentity {
    pub(in crate::pending_work) project: Project,
    pub(in crate::pending_work) identifier: WorkItemId,
}

pub(in crate::pending_work) struct PreparedPendingWorkUpdate {
    pub(in crate::pending_work) identity: PendingWorkItemIdentity,
    pub(in crate::pending_work) patch: ItemPatch,
    pub(in crate::pending_work) outcome: super::update_pending_work_item::UpdatePendingWorkItemOk,
}
