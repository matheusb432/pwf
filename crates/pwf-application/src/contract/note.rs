use std::num::NonZeroUsize;

use pwf_models::{
    AppDate,
    note::{
        NoteContent, NoteDomain, NoteId, NoteSelector, NoteSource, NoteTag, NoteTitle,
        NoteVerification, NoteWhy,
    },
    project::{ProjectId, ProjectName, ProjectSelector},
};

use super::project::ResolveProjectApiError;

/// Requests creation of one project note.
#[derive(Debug, Clone)]
pub struct AddNote {
    /// Project's name or id.
    pub project_selector: ProjectSelector,
    /// Names the note.
    pub title: NoteTitle,
    /// Supplies the note's Markdown body.
    pub content: NoteContent,
    /// Explains the consequence when it adds useful context.
    pub why: Option<NoteWhy>,
    /// Classifies the subject when known.
    pub domain: Option<NoteDomain>,
    /// Supplies discovery labels.
    pub tags: Vec<NoteTag>,
    /// Records supporting evidence.
    pub sources: Vec<NoteSource>,
    /// Records the supplied verification marker.
    pub verified: Option<NoteVerification>,
    /// Overrides the clock date when present.
    pub date: Option<AppDate>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AddNoteApiError {
    #[error("Positional note must contain ' / ' between its title and content.")]
    MissingSeparator,
    #[error("{message}")]
    InvalidTitle { message: String },
    #[error("{message}")]
    InvalidContent { message: String },
    #[error("Provide either '<title> / <content>' or both --title and --content.")]
    InvalidRequest,
    #[error(transparent)]
    ResolveProject(#[from] ResolveProjectApiError),
    #[error("Project '{project}' has no available four-digit note identifiers.")]
    IdentifierExhausted { project: ProjectName },
    #[error("{message}")]
    Unexpected { message: String },
}

/// Requests one project's notes in newest-first order.
#[derive(Debug, Clone)]
pub struct ListNotes {
    /// Selects the managed project by name or id code.
    pub project_selector: ProjectSelector,
    /// Caps returned notes.
    pub limit: NoteListLimit,
}

/// Selects the default cap, no cap, or an explicit non-zero cap.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum NoteListLimit {
    #[default]
    Default,
    Unlimited,
    AtMost(NonZeroUsize),
}

impl From<Option<usize>> for NoteListLimit {
    fn from(number: Option<usize>) -> Self {
        match number.and_then(NonZeroUsize::new) {
            Some(number) => Self::AtMost(number),
            None if number.is_some() => Self::Unlimited,
            None => Self::Default,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ListNotesApiError {
    #[error(transparent)]
    ResolveProject(#[from] ResolveProjectApiError),
    #[error("{message}")]
    Unexpected { message: String },
}

/// Requests deletion of one project note.
#[derive(Debug, Clone)]
pub struct RemoveNote {
    /// Selects the managed project by name or id code.
    pub project_selector: ProjectSelector,
    /// Selects the note by full id, `NOTE-NNNN`, or bare numeric suffix.
    pub selector: NoteSelector,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RemoveNoteApiError {
    #[error(transparent)]
    ResolveProject(#[from] ResolveProjectApiError),
    #[error("Note id '{selector}' does not belong to project {project_id}.")]
    ProjectMismatch {
        selector: NoteSelector,
        project_id: ProjectId,
    },
    #[error("No such note {id} in {project}.")]
    NoSuchNote { id: NoteId, project: ProjectName },
    #[error("{message}")]
    Unexpected { message: String },
}

/// Requests replacement of one project note's title.
#[derive(Debug, Clone)]
pub struct UpdateNote {
    /// Selects the managed project by name or id code.
    pub project_selector: ProjectSelector,
    /// Selects the note by full id, `NOTE-NNNN`, or bare numeric suffix.
    pub selector: NoteSelector,
    /// Supplies the replacement title.
    pub title: NoteTitle,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UpdateNoteApiError {
    #[error(transparent)]
    ResolveProject(#[from] ResolveProjectApiError),
    #[error("Note id '{selector}' does not belong to project {project_id}.")]
    ProjectMismatch {
        selector: NoteSelector,
        project_id: ProjectId,
    },
    #[error("No such note {id} in {project}.")]
    NoSuchNote { id: NoteId, project: ProjectName },
    #[error("{message}")]
    InvalidTitle { message: String },
    #[error("{message}")]
    Unexpected { message: String },
}

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
