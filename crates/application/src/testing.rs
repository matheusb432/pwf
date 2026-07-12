use std::convert::Infallible;

use pwf_domain::pending_work::{AddedItem, OpenItem, ProjectName, RemovedItem, UpdatedItem};

use crate::ports::{
    AddItemSpec, CancelItemSpec, ClosedItem, ClosedItemAction, CompleteItemSpec,
    PendingWorkReadStore, PendingWorkWriteStore, ReopenedItem, StatusTransitionDiagnostics,
    UpdateItemSpec,
};

#[derive(Debug, Clone, Default)]
pub struct InMemoryPendingWorkReadStore {
    items: Vec<OpenItem>,
}

impl InMemoryPendingWorkReadStore {
    pub fn with_items(items: Vec<OpenItem>) -> Self {
        Self { items }
    }
}

impl PendingWorkReadStore for InMemoryPendingWorkReadStore {
    type Error = Infallible;

    fn open_items_for_project(&self, project: &ProjectName) -> Result<Vec<OpenItem>, Self::Error> {
        Ok(self
            .items
            .iter()
            .filter(|item| item.project == project.as_ref())
            .cloned()
            .collect())
    }

    fn all_open_items(&self) -> Result<Vec<OpenItem>, Self::Error> {
        Ok(self.items.clone())
    }

    fn open_item(&self, id: &str) -> Result<OpenItem, Self::Error> {
        Ok(self
            .items
            .iter()
            .find(|item| item.id.eq_ignore_ascii_case(id))
            .expect("in-memory test query references a staged item")
            .clone())
    }
}

#[derive(Debug, Clone)]
pub struct InMemoryPendingWorkWriteStore {
    added: AddedItem,
    updated: UpdatedItem,
    removed: RemovedItem,
}

impl InMemoryPendingWorkWriteStore {
    pub fn new(added: AddedItem, updated: UpdatedItem, removed: RemovedItem) -> Self {
        Self {
            added,
            updated,
            removed,
        }
    }
}

impl PendingWorkWriteStore for InMemoryPendingWorkWriteStore {
    type Error = Infallible;

    fn add_item(&self, _spec: AddItemSpec) -> Result<AddedItem, Self::Error> {
        Ok(self.added.clone())
    }

    fn update_item(&self, _spec: UpdateItemSpec) -> Result<UpdatedItem, Self::Error> {
        Ok(self.updated.clone())
    }

    fn remove_item(&self, _id: &str) -> Result<RemovedItem, Self::Error> {
        Ok(self.removed.clone())
    }

    fn complete_item(&self, _spec: CompleteItemSpec) -> Result<ClosedItem, Self::Error> {
        Ok(ClosedItem {
            id: "PWF-0001".to_string(),
            project: "pwf".to_string(),
            title: "done".to_string(),
            action: ClosedItemAction::Done,
            diagnostics: StatusTransitionDiagnostics::none(),
        })
    }

    fn cancel_item(&self, _spec: CancelItemSpec) -> Result<ClosedItem, Self::Error> {
        Ok(ClosedItem {
            id: "PWF-0001".to_string(),
            project: "pwf".to_string(),
            title: "cancelled".to_string(),
            action: ClosedItemAction::Cancelled,
            diagnostics: StatusTransitionDiagnostics::none(),
        })
    }

    fn reopen_item(&self, id: &str) -> Result<ReopenedItem, Self::Error> {
        Ok(ReopenedItem {
            id: id.to_string(),
            project: "pwf".to_string(),
            already_active: false,
        })
    }
}
