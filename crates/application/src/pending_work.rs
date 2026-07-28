pub mod add_pending_work_item;
pub mod cancel_pending_work;
mod commit_provenance;
pub mod complete_pending_work;
mod enrich;
pub mod find_pending_work;
pub mod get_pending_work;
pub(crate) mod identifier;
mod note_body;
mod prerequisite;
mod project_registry;
pub mod reject_pending_work_create;
pub mod remove_pending_work_item;
pub mod reopen_pending_work;
mod resolve;
mod section;
pub mod session;
pub mod show_pending_work_item;
pub(crate) mod store_util;
pub(crate) mod tag_policy;
mod title;
pub mod update_pending_work_item;

pub use add_pending_work_item::AddPendingWorkItemOk;
pub use get_pending_work::{
    GetPendingWorkOk, ListMode, ListSection, OrderDirection, OrderField, OrderSpec,
    PendingWorkItemView, PrerequisiteStatus, StatusFilter,
};
pub use project_registry::{ProjectRegistry, ProjectResolutionError};
pub use remove_pending_work_item::RemovedItem;
pub use show_pending_work_item::ShowOutput;
pub use update_pending_work_item::UpdatePendingWorkItemOk;

/// Renders a prompt as Markdown while preserving placeholders and verbatim-authored prompts.
///
/// # Examples
///
/// ```
/// use pwf_application::pending_work::note_body;
///
/// assert_eq!(note_body("ship release"), "## Goals\n\n- ship release");
/// ```
#[must_use]
pub fn note_body(prompt: &str) -> String {
    note_body::render(prompt)
}
