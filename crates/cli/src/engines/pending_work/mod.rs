//! Pending-work engine, split by responsibility. `mod.rs` only wires the
//! submodules together and re-exports the public surface; behavior lives in the
//! focused submodules below.

mod actions;
mod agent;
mod color;
mod commits;
mod continue_prompt;
mod domain;
mod effort;
mod errors;
mod handoff_query;
mod index;
mod launch;
mod model;
mod naming;
mod new_add;
mod obsidian;
mod prereq;
mod prompt_format;
mod query;
mod render;
mod route;
mod run;
mod section;
mod session;
mod tags;
mod text;

pub(crate) use actions::{
    emit_created_section_diagnostic, emit_created_section_diagnostic_for_error,
};
pub use domain::{commands::PendingWorkCommand, types::canonical_pending_id};
pub use index::{
    WorkItemFields, add_link_to_index, add_section_block, find_section_index, remove_index_link,
    section_exists, set_status_text, work_item_content,
};
pub(crate) use model::Action;
pub use model::Item;
pub use naming::{project_dir, project_index_path, project_key, stamp_date};
pub use query::{is_item_open, resolve_managed_project_name};
pub(crate) use run::add_command_from_args;
// Only test fixtures outside this module (`handoff::mirror`/`pw_bridge`'s own
// `#[cfg(test)]` blocks) need `store_for` directly; production code either
// lives inside this module (plain `store_for(...)`) or goes through
// `add_command_from_args`, so the cross-module re-export is test-only.
#[cfg(test)]
pub(crate) use run::{project_registry, store_for};
pub use run::{run, run_args};
pub use text::{
    get_title_from_continue_path, goals_body, handoff_title_from_path, inferred_title,
    is_placeholder_prompt, line_number, note_body, project_title_prefix,
};
