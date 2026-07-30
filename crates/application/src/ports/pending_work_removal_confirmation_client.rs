use crate::pending_work::remove_pending_work_item::RemovalConfirmation;

pub trait PendingWorkRemovalConfirmationClient: Clone + Send + Sync + 'static {
    fn confirm(&self, confirmation: &RemovalConfirmation) -> bool;
}
