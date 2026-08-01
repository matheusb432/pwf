pub mod add_pending_work_item;
pub mod cancel_pending_work;
pub mod complete_pending_work;
mod dto;
pub mod find_pending_work;
pub mod get_pending_work;
mod logic;
pub mod reject_pending_work_create;
pub mod remove_pending_work_item;
pub mod reopen_pending_work;
pub mod session;
pub mod show_pending_work_item;
pub mod update_pending_work_item;

pub use add_pending_work_item::AddPendingWorkItemOk;
pub use dto::{PendingWorkItemView, PrerequisiteStatus};
pub use get_pending_work::{
    GetPendingWorkOk, ListMode, ListSection, OrderDirection, OrderField, OrderSpec, StatusFilter,
};
#[cfg(test)]
use logic::resolve;
use logic::{enrich, identifier, note_body, prerequisite, section, store_util, tag_policy, title};
pub use remove_pending_work_item::RemovedItem;
pub use show_pending_work_item::ShowOutput;
pub use update_pending_work_item::UpdatePendingWorkItemOk;

pub use crate::project::{ProjectRegistry, ProjectResolutionError};

/// Renders a prompt as Markdown while preserving placeholders and verbatim-authored prompts.
#[must_use]
pub fn note_body(prompt: &str) -> String {
    note_body::render(prompt)
}
