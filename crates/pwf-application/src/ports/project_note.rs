use pwf_models::{
    note::{NoteId, ProjectNote},
    pending_work::{ProjectName, Timestamp},
};

use super::app_record::{AppRecordStore, Record};

impl Record for ProjectNote {
    type Scope = ProjectName;
    type Id = NoteId;
    type New = NewProjectNote;
    type Patch = ProjectNotePatch;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewProjectNote {
    pub id: NoteId,
    pub topic: String,
    pub tldr: String,
    pub why: Option<String>,
    pub domain: Option<String>,
    pub tags: Vec<String>,
    pub sources: Vec<String>,
    pub verified: Option<String>,
    pub created: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectNotePatch {
    pub topic: String,
}

pub trait ProjectNoteStore: AppRecordStore<ProjectNote> {
    fn note_exists(
        &self,
        project: &ProjectName,
        id: &NoteId,
    ) -> Result<bool, <Self as AppRecordStore<ProjectNote>>::Error>;

    fn read_note_markdown(
        &self,
        locator: &str,
    ) -> Result<String, <Self as AppRecordStore<ProjectNote>>::Error>;
}
