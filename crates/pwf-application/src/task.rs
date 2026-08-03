pub mod add_task;
pub mod cancel_task;
pub mod complete_task;
mod dto;
pub mod find_active_task;
pub mod list_tasks;
mod logic;
pub mod remove_task;
pub mod reopen_task;
pub mod resolve_task_project;
pub mod session;
pub mod show_task;
pub mod update_task;

pub use add_task::AddTaskOk;
pub use dto::{PrerequisiteStatus, TaskView};
pub use list_tasks::{
    ListMode, ListSection, ListTasksOk, OrderDirection, OrderField, OrderSpec, StatusFilter,
};
#[cfg(test)]
use logic::resolve;
use logic::{enrich, identifier, note_body, prerequisite, section, store_util, tag_policy, title};
pub use remove_task::RemovedTask;
pub use show_task::ShowOutput;
pub use update_task::UpdateTaskOk;

/// Renders a prompt as Markdown while preserving placeholders and verbatim-authored prompts.
#[must_use]
pub fn note_body(prompt: &str) -> String {
    note_body::render(prompt)
}
