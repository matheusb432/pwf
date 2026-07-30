//! Defines shared project-note read DTOs.

use pwf_models::note::NoteId;

/// Describes one note returned by a project-note read.
///
/// # Examples
///
/// ```
/// use pwf_application::note::dto::ListedNote;
/// use pwf_models::note::NoteId;
///
/// let note = ListedNote {
///     id: NoteId::try_new("PWF-NOTE-0001").unwrap(),
///     topic: "remember milk".to_string(),
/// };
/// assert_eq!(note.topic, "remember milk");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedNote {
    /// Identifies the note within its project.
    pub id: NoteId,
    /// Names the focused learning topic.
    pub topic: String,
}
