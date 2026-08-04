//! Defines project-note interactors.

pub mod add_note;
pub mod list_notes;
pub mod remove_note;
pub mod update_note;

use pwf_models::{note::NoteId, project::ProjectId};

fn resolve_note(raw: &str, project_id: &ProjectId) -> Option<NoteId> {
    let raw_uppercase = raw.trim().to_ascii_uppercase();
    let full_prefix = format!("{project_id}-NOTE-");
    let number = raw_uppercase
        .strip_prefix(&full_prefix)
        .or_else(|| raw_uppercase.strip_prefix("NOTE-"))
        .unwrap_or(&raw_uppercase)
        .parse::<u32>()
        .ok()?;
    NoteId::try_new(format!("{project_id}-NOTE-{number:04}")).ok()
}
