use std::path::Path;

pub trait NoteMarkdownClient: Clone + Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;

    fn read_note_markdown(&self, path: &Path) -> Result<String, Self::Error>;
}
