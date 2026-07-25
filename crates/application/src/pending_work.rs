pub mod add;
pub mod cancel;
mod commit_provenance;
pub mod done;
mod enrich;
pub mod find;
pub(crate) mod identifier;
pub mod list;
mod note_body;
mod prerequisite;
mod project_registry;
pub mod remove;
pub mod reopen;
mod resolve;
mod section;
pub mod session;
pub mod show;
pub(crate) mod store_util;
pub(crate) mod tag_policy;
mod title;
pub mod update;

pub use add::{AddPendingWorkSource, AddedItem, PendingWorkSection};
pub use list::{
    ListMode, ListResult, ListSection, OrderDirection, OrderField, OrderSpec, PendingWorkItemView,
    PrerequisiteStatus, StatusFilter,
};
pub use project_registry::{ProjectRegistry, ProjectResolutionError};
pub use remove::RemovedItem;
pub use show::ShowOutput;
pub use update::UpdatePendingWorkItemOk;

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
