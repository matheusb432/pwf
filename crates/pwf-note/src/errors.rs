//! Note-engine error boundary.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum NoteError {
    /// The project is not mapped to an id prefix in config.
    #[error("Project '{0}' has no configured prefix; add it to config/pending-work.json.")]
    UnknownProject(String),
    /// The supplied note id could not be parsed for this project.
    #[error("Invalid note id '{0}'; expected e.g. {1}-NOTE-0001, NOTE-0001, or 1.")]
    BadId(String, String),
    /// No note with this id exists in the project.
    #[error("No such note {id} in {project}.")]
    NoSuchNote { id: String, project: String },
    /// The supplied note message is empty or whitespace-only.
    #[error("Note message is empty; provide a non-empty message.")]
    EmptyMessage,
    /// A filesystem operation failed.
    #[error("{context}: {source}")]
    Io {
        context: String,
        source: std::io::Error,
    },
}

impl From<NoteError> for String {
    fn from(e: NoteError) -> Self {
        e.to_string()
    }
}
