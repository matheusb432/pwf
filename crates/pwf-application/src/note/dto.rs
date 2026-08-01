//! Defines shared project-note read DTOs.

use pwf_models::note::NoteId;

/// Describes one note returned by a project-note read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedNote {
    /// Identifies the note within its project.
    pub id: NoteId,
    /// Names the focused learning topic.
    pub topic: String,
}
