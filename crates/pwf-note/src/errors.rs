//! Note-engine failures.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum NoteError {
    #[error("Project '{0}' has no configured prefix; add it to config/pending-work.json.")]
    UnknownProject(String),
    #[error("Invalid note id '{0}'; expected e.g. {1}-NOTE-0001, NOTE-0001, or 1.")]
    BadId(String, String),
    #[error("No such note {id} in {project}.")]
    NoSuchNote { id: String, project: String },
    #[error("Note message is empty; provide a non-empty message.")]
    EmptyMessage,
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
