// Compatibility facade for note/index text transforms.

pub use super::obsidian::{
    index_text::{
        add_link_to_index, add_section_block, find_section_index, remove_index_link, section_exists,
    },
    note_text::{
        append_report_text, set_commits_text, set_prereq_text, set_status_text, work_item_content,
    },
};
