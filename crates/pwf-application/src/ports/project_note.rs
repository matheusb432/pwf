use pwf_models::{
    AppDate,
    note::{
        NoteContent, NoteDomain, NoteId, NoteSource, NoteTag, NoteTitle, NoteVerification,
        ProjectNote,
    },
    project::Project,
};
use pwf_wire::{collection_edit::CollectionEdit, patch_field::PatchField, set_field::SetField};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewProjectNote {
    pub id: NoteId,
    pub title: NoteTitle,
    pub content: NoteContent,
    pub domain: Option<NoteDomain>,
    pub tags: Vec<NoteTag>,
    pub sources: Vec<NoteSource>,
    pub verified: Option<NoteVerification>,
    pub created: AppDate,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectNotePatch {
    pub title: SetField<NoteTitle>,
    pub content: SetField<NoteContent>,
    pub domain: PatchField<NoteDomain>,
    pub tags: CollectionEdit<Vec<NoteTag>>,
    pub sources: CollectionEdit<Vec<NoteSource>>,
    pub verified: PatchField<NoteVerification>,
}

/// Persists project notes independently of task-note storage.
pub trait ProjectNotes: Send + Sync + 'static {
    type Error: std::error::Error + Send + Sync + 'static;

    fn get_note(&self, project: &Project, id: &NoteId) -> Result<Option<ProjectNote>, Self::Error>;
    fn list_notes(&self, project: &Project) -> Result<Vec<ProjectNote>, Self::Error>;
    fn insert_note(
        &self,
        project: &Project,
        note: NewProjectNote,
    ) -> Result<ProjectNote, Self::Error>;
    fn update_note(
        &self,
        project: &Project,
        id: &NoteId,
        patch: ProjectNotePatch,
    ) -> Result<(), Self::Error>;
    fn delete_note(&self, project: &Project, id: &NoteId) -> Result<(), Self::Error>;
}
