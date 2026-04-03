//! Pending-work engine, split by responsibility. `mod.rs` only wires the
//! submodules together and re-exports the public surface; behavior lives in the
//! focused submodules below.

mod actions;
mod add;
mod claude;
mod commits;
mod continue_prompt;
mod done_queue;
mod errors;
mod index;
mod launch;
mod model;
mod naming;
mod new_add;
mod parse;
mod prereq;
mod query;
mod route;
mod run;
mod section;
mod text;

pub use claude::{ClaudeProbe, FakeProbe, RealProbe, verify_json_with_probe};
pub use index::{
    add_link_to_index, add_section_block, find_section_index, remove_index_link, section_exists,
    set_status_text, work_item_content,
};
pub use model::Item;
pub use naming::{next_work_item_id, project_dir, project_index_path, project_key, stamp_date};
pub use parse::{get_project_tasks, newest_handoff};
pub use query::{is_item_open, resolve_managed_project_name};
pub use run::run;
pub use text::{
    get_title_from_continue_path, goals_body, handoff_title_from_path, inferred_title,
    is_placeholder_prompt, line_number, note_body, project_title_prefix,
};
