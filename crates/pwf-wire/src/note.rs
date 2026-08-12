use pwf_models::{
    note::{NoteId, NoteTitle},
    project::ProjectName,
};

/// Describes one newly persisted project note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddedNote {
    pub id: NoteId,
    pub title: NoteTitle,
}

/// Describes one note returned by a project-note read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedNote {
    /// Identifies the note within its project.
    pub id: NoteId,
    /// Names the note.
    pub title: NoteTitle,
}

/// Describes one capped project-note listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedNotes {
    pub project: ProjectName,
    pub notes: Vec<ListedNote>,
    pub hidden: usize,
}

/// Describes one removed project note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovedNote {
    pub id: NoteId,
}

/// Describes one project note after its title changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdatedNote {
    pub id: NoteId,
    pub title: NoteTitle,
}
