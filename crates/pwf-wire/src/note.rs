use std::num::NonZeroUsize;

use pwf_models::{
    AppDate,
    note::{
        NoteContent, NoteDomain, NoteId, NoteSelector, NoteSource, NoteTag, NoteTitle,
        NoteVerification, ProjectNote,
    },
    project::{ProjectName, ProjectSelector},
};

use crate::{collection_edit::CollectionEdit, patch_field::PatchField, set_field::SetField};

/// Requests creation of one project note.
#[derive(Debug, Clone)]
pub struct AddNote {
    /// Project's name or id.
    pub project_selector: ProjectSelector,
    /// Names the note.
    pub title: NoteTitle,
    /// Supplies the note's Markdown body.
    pub content: NoteContent,
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

/// Carries at least one requested note change.
#[derive(Debug, Clone)]
pub struct NoteEdits {
    title: SetField<NoteTitle>,
    content: SetField<NoteContent>,
    domain: PatchField<NoteDomain>,
    tags: CollectionEdit<Vec<NoteTag>>,
    sources: CollectionEdit<Vec<NoteSource>>,
    verified: PatchField<NoteVerification>,
}

impl NoteEdits {
    /// Creates a non-empty set of note changes.
    ///
    /// # Errors
    ///
    /// Returns [`EmptyNoteEdits`] when every field is unchanged.
    pub fn try_new(
        title: SetField<NoteTitle>,
        content: SetField<NoteContent>,
        domain: PatchField<NoteDomain>,
        tags: CollectionEdit<Vec<NoteTag>>,
        sources: CollectionEdit<Vec<NoteSource>>,
        verified: PatchField<NoteVerification>,
    ) -> Result<Self, EmptyNoteEdits> {
        if title.is_unchanged()
            && content.is_unchanged()
            && domain.is_unchanged()
            && tags.is_unchanged()
            && sources.is_unchanged()
            && verified.is_unchanged()
        {
            return Err(EmptyNoteEdits);
        }
        Ok(Self {
            title,
            content,
            domain,
            tags,
            sources,
            verified,
        })
    }

    #[must_use]
    pub fn title(&self) -> &SetField<NoteTitle> {
        &self.title
    }

    #[must_use]
    pub fn content(&self) -> &SetField<NoteContent> {
        &self.content
    }

    #[must_use]
    pub fn domain(&self) -> &PatchField<NoteDomain> {
        &self.domain
    }

    #[must_use]
    pub fn tags(&self) -> &CollectionEdit<Vec<NoteTag>> {
        &self.tags
    }

    #[must_use]
    pub fn sources(&self) -> &CollectionEdit<Vec<NoteSource>> {
        &self.sources
    }

    #[must_use]
    pub fn verified(&self) -> &PatchField<NoteVerification> {
        &self.verified
    }
}

/// Reports an edit request with no changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("nothing to edit; pass at least one edit flag")]
pub struct EmptyNoteEdits;

/// Requests a partial edit of one project note.
#[derive(Debug, Clone)]
pub struct EditNote {
    /// Selects the managed project by name or id code.
    pub project_selector: ProjectSelector,
    /// Selects the note by full id, `NOTE-NNNN`, or bare numeric suffix.
    pub selector: NoteSelector,
    /// Selects explicit changes while preserving every omitted field.
    pub edits: NoteEdits,
}

/// Identifies one note mutation for frontend feedback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutatedNote {
    pub id: NoteId,
    pub project: ProjectName,
    pub title: NoteTitle,
}

/// Reports whether a confirmed note removal completed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemovedNoteOutcome {
    Removed(MutatedNote),
    Aborted { note_id: NoteId },
}

/// Describes one capped project-note listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedNotes {
    pub project: ProjectName,
    pub notes: Vec<ProjectNote>,
    pub hidden: usize,
}
