use pwf_application::{
    AddItemSpec, CancelItemSpec, ClosedItem, CompleteItemSpec, PendingWorkWriteStore, ReopenedItem,
    UpdateItemSpec,
};
use pwf_domain::pending_work::{AddedItem, RemovedItem, UpdatedItem};

use super::{
    ObsidianPendingWorkStore, ObsidianPendingWorkStoreError,
    status::{CloseItemKind, CloseItemSpec},
};

impl PendingWorkWriteStore for ObsidianPendingWorkStore {
    type Error = ObsidianPendingWorkStoreError;

    fn add_item(&self, spec: AddItemSpec) -> Result<AddedItem, Self::Error> {
        self.add_item_impl(spec)
    }

    fn update_item(&self, spec: UpdateItemSpec) -> Result<UpdatedItem, Self::Error> {
        self.update_item_impl(&spec)
    }

    fn remove_item(&self, id: &str) -> Result<RemovedItem, Self::Error> {
        self.remove_item_impl(id)
    }

    fn complete_item(&self, spec: CompleteItemSpec) -> Result<ClosedItem, Self::Error> {
        self.close_item(&CloseItemSpec {
            id: spec.id,
            completed: spec.completed,
            commits: spec.commits,
            kind: CloseItemKind::Done {
                report: spec.report,
            },
        })
    }

    fn cancel_item(&self, spec: CancelItemSpec) -> Result<ClosedItem, Self::Error> {
        if spec.report.trim().is_empty() {
            return Err(ObsidianPendingWorkStoreError::EmptyReport);
        }
        self.close_item(&CloseItemSpec {
            id: spec.id,
            completed: spec.completed,
            commits: spec.commits,
            kind: CloseItemKind::Cancelled {
                report: spec.report,
            },
        })
    }

    fn reopen_item(&self, id: &str) -> Result<ReopenedItem, Self::Error> {
        self.reopen_item_impl(id)
    }
}
