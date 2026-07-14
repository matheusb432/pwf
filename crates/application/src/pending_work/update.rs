use pwf_domain::pending_work::{Tags, UpdatedItem};

use crate::ports::{PendingWorkWriteStore, UpdateItemSpec};

#[derive(Debug, Clone)]
pub struct UpdatePendingWorkItem {
    pub id: String,
    pub prompt: Option<String>,
    pub title: Option<String>,
    pub append: Option<String>,
    pub prereq: Vec<String>,
    pub clear_prereq: bool,
    pub commits: Option<String>,
    pub append_report: Option<String>,
    pub effort: Option<u8>,
    pub tags: Option<Tags>,
    pub tags_clear: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum UpdatePendingWorkError {
    #[error("{0}")]
    WriteStore(Box<dyn std::error::Error + Send + Sync>),
}

#[cqrsy::handler(command)]
pub fn execute(
    cmd: UpdatePendingWorkItem,
    store: &impl PendingWorkWriteStore,
) -> Result<UpdatedItem, UpdatePendingWorkError> {
    store
        .update_item(UpdateItemSpec {
            id: cmd.id,
            prompt: cmd.prompt,
            title: cmd.title,
            append: cmd.append,
            prereq: cmd.prereq,
            clear_prereq: cmd.clear_prereq,
            commits: cmd.commits,
            append_report: cmd.append_report,
            effort: cmd.effort,
            tags: cmd.tags,
            tags_clear: cmd.tags_clear,
        })
        .map_err(|error| UpdatePendingWorkError::WriteStore(Box::new(error)))
}
