use pwf_domain::pending_work::{AddedItem, Tags};

use crate::ports::{AddItemSpec, PendingWorkWriteStore};

#[derive(Debug, Clone)]
pub struct AddPendingWorkItem {
    pub project_name: String,
    pub prompt: String,
    pub title: Option<String>,
    pub created: String,
    pub section: Option<String>,
    pub prereq: Option<String>,
    pub effort: Option<u8>,
    pub tags: Option<Tags>,
}

#[derive(Debug, thiserror::Error)]
pub enum AddPendingWorkError {
    #[error("{0}")]
    WriteStore(Box<dyn std::error::Error + Send + Sync>),
}

#[cqrsy::handler(command)]
pub fn handle(
    store: &impl PendingWorkWriteStore,
    cmd: AddPendingWorkItem,
) -> Result<AddedItem, AddPendingWorkError> {
    store
        .add_item(AddItemSpec {
            project_name: cmd.project_name,
            prompt: cmd.prompt,
            title: cmd.title,
            created: cmd.created,
            section: cmd.section,
            prereq: cmd.prereq,
            effort: cmd.effort,
            tags: cmd.tags,
        })
        .map_err(|error| AddPendingWorkError::WriteStore(Box::new(error)))
}
