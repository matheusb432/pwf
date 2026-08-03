use pwf_models::{
    note::{NoteId, ProjectNote},
    project::Project,
    task::Timestamp,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewProjectNote {
    pub id: NoteId,
    pub title: String,
    pub content: String,
    pub why: Option<String>,
    pub domain: Option<String>,
    pub tags: Vec<String>,
    pub sources: Vec<String>,
    pub verified: Option<String>,
    pub created: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectNotePatch {
    pub title: String,
}

pub trait ProjectNoteStore: Clone + Send + Sync + 'static {
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
    fn note_exists(&self, project: &Project, id: &NoteId) -> Result<bool, Self::Error>;

    fn read_note_markdown(&self, locator: &str) -> Result<String, Self::Error>;
}
