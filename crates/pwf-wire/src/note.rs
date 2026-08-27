use std::num::NonZeroUsize;

use pwf_models::{
    AppDate,
    note::{
        NoteContent, NoteDomain, NoteId, NoteSelector, NoteSource, NoteTag, NoteTitle,
        NoteVerification, NoteWhy,
    },
    project::{ProjectName, ProjectSelector},
};

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

/// Requests deletion of one project note.
#[derive(Debug, Clone)]
pub struct RemoveNote {
    /// Selects the managed project by name or id code.
    pub project_selector: ProjectSelector,
    /// Selects the note by full id, `NOTE-NNNN`, or bare numeric suffix.
    pub selector: NoteSelector,
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

/// Identifies and names one project note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteSummary {
    pub id: NoteId,
    pub title: NoteTitle,
}

/// Describes one capped project-note listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedNotes {
    pub project: ProjectName,
    pub notes: Vec<NoteSummary>,
    pub hidden: usize,
}
