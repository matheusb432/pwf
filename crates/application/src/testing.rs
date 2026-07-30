use std::{
    collections::BTreeMap,
    convert::Infallible,
    sync::{Arc, Mutex, MutexGuard},
};

use pwf_models::{
    note::{NoteId, ProjectNote},
    pending_work::{ProjectName, Timestamp, WorkItemId, WorkItemStatus},
};

use crate::ports::{
    AppRecordStore, IndexEntry, IndexSection, ItemPatch, Materialization, NewItem, NewProjectNote,
    PendingWorkRecord, ProjectNotePatch, ProjectNoteStore, RecordId,
};

#[derive(Debug, Default)]
struct InMemoryState {
    items: BTreeMap<ProjectName, Vec<PendingWorkRecord>>,
    entries: BTreeMap<ProjectName, Vec<IndexEntry>>,
    sections: BTreeMap<ProjectName, Vec<String>>,
    prefixes: BTreeMap<ProjectName, String>,
    project_notes: BTreeMap<ProjectName, Vec<ProjectNote>>,
    project_note_creations: BTreeMap<ProjectName, Vec<Timestamp>>,
    project_note_failures: Vec<ProjectNoteFailure>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProjectNoteFailure {
    Delete,
    List,
    Read,
}

/// Provides a thread-safe [`AppRecordStore`] test double for application records.
///
/// Clones share one `Arc<Mutex<InMemoryState>>` and therefore observe the same writes.
#[derive(Debug, Clone, Default)]
pub struct InMemoryStore {
    state: Arc<Mutex<InMemoryState>>,
}

/// Reports rejected writes in the application test store.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InMemoryStoreError {
    #[error("index sections are managed implicitly and cannot be written directly (op: {op})")]
    IndexSectionWriteUnsupported { op: &'static str },
    #[error("injected in-memory store failure: {operation}")]
    Injected { operation: &'static str },
    #[error("pending-work note Markdown is not staged: {locator}")]
    PendingWorkNoteMarkdownMissing { locator: String },
}

impl InMemoryStore {
    pub fn with_project(self, project: &str, items: Vec<PendingWorkRecord>) -> Self {
        self.lock().items.insert(project_name(project), items);
        self
    }

    /// Registers the id prefix used by item insertion.
    pub fn with_prefix(self, project: &str, prefix: &str) -> Self {
        self.lock()
            .prefixes
            .insert(project_name(project), prefix.to_string());
        self
    }

    /// Stages index sections by raw H2 label.
    pub fn with_sections(self, project: &str, labels: &[&str]) -> Self {
        self.lock().sections.insert(
            project_name(project),
            labels.iter().map(|label| (*label).to_string()).collect(),
        );
        self
    }

    pub fn items(&self, project: &str) -> Vec<PendingWorkRecord> {
        self.lock()
            .items
            .get(&project_name(project))
            .cloned()
            .unwrap_or_default()
    }

    pub fn entries(&self, project: &str) -> Vec<IndexEntry> {
        self.lock()
            .entries
            .get(&project_name(project))
            .cloned()
            .unwrap_or_default()
    }

    pub fn with_project_notes(self, project: &str, notes: Vec<ProjectNote>) -> Self {
        self.lock()
            .project_notes
            .insert(project_name(project), notes);
        self
    }

    pub fn project_notes(&self, project: &str) -> Vec<ProjectNote> {
        self.lock()
            .project_notes
            .get(&project_name(project))
            .cloned()
            .unwrap_or_default()
    }

    pub fn project_note_creations(&self, project: &str) -> Vec<Timestamp> {
        self.lock()
            .project_note_creations
            .get(&project_name(project))
            .cloned()
            .unwrap_or_default()
    }

    pub(crate) fn with_failure(self, failure: ProjectNoteFailure) -> Self {
        self.lock().project_note_failures.push(failure);
        self
    }

    fn lock(&self) -> MutexGuard<'_, InMemoryState> {
        self.state.lock().expect("in-memory store lock poisoned")
    }
}

fn project_name(project: &str) -> ProjectName {
    ProjectName::try_new(project).expect("test project name is non-empty")
}

impl AppRecordStore<PendingWorkRecord> for InMemoryStore {
    type Error = Infallible;

    fn get(
        &self,
        project: &ProjectName,
        id: &WorkItemId,
    ) -> Result<Option<PendingWorkRecord>, Self::Error> {
        Ok(self.lock().items.get(project).and_then(|items| {
            items
                .iter()
                .find(|item| item.id.as_item() == Some(id))
                .cloned()
        }))
    }

    fn list(&self, project: &ProjectName) -> Result<Vec<PendingWorkRecord>, Self::Error> {
        Ok(self.lock().items.get(project).cloned().unwrap_or_default())
    }

    /// Allocates the next `<prefix>-NNNN` id and materializes an active record.
    fn insert(
        &self,
        project: &ProjectName,
        new: NewItem,
    ) -> Result<PendingWorkRecord, Self::Error> {
        let mut state = self.lock();
        let prefix = state
            .prefixes
            .get(project)
            .cloned()
            .expect("stage a prefix via with_prefix before insert");
        let items = state.items.entry(project.clone()).or_default();
        let next = items
            .iter()
            .filter_map(|item| item.id.as_item())
            .filter_map(|id| id.as_ref().split_once('-'))
            .filter_map(|(_, number)| number.parse::<u32>().ok())
            .max()
            .unwrap_or(0)
            + 1;
        let id = WorkItemId::try_new(format!("{prefix}-{next:04}")).expect("allocated id");
        let locator = format!("/mem/{}/{}.md", project.as_ref(), id.as_ref());
        let record = PendingWorkRecord {
            id: RecordId::Item(id),
            title: new.title.to_string(),
            status: WorkItemStatus::Active,
            created: Some(new.created),
            completed: None,
            commits: None,
            tags: new.tags.map(|tags| render_tags(&tags)),
            effort: new.effort.map(|effort| effort.to_string()),
            prereq: new.prereq,
            section: None,
            body: new.prompt.clone(),
            source: new.prompt,
            locator,
            placement: None,
            materialization: Materialization::NoteFile,
        };
        items.push(record.clone());
        Ok(record)
    }

    /// Applies an [`ItemPatch`] to the matching record's typed fields.
    fn update(
        &self,
        project: &ProjectName,
        id: &WorkItemId,
        patch: ItemPatch,
    ) -> Result<(), Self::Error> {
        let mut state = self.lock();
        let items = state.items.entry(project.clone()).or_default();
        let record = items
            .iter_mut()
            .find(|item| item.id.as_item() == Some(id))
            .expect("update of unknown id");
        if let Some(status) = patch.status {
            record.status = status;
        }
        if let Some(completed) = patch.completed {
            record.completed = completed;
        }
        if let Some(commits) = patch.commits {
            record.commits = commits;
        }
        if let Some(body) = patch.body {
            record.body = body;
        }
        if let Some(title) = patch.title {
            record.title = title.to_string();
        }
        if let Some(prereq) = patch.prereq {
            record.prereq = prereq;
        }
        if let Some(effort) = patch.effort {
            record.effort = Some(effort.to_string());
        }
        if let Some(tags) = patch.tags {
            record.tags = tags.map(|tags| render_tags(&tags));
        }
        Ok(())
    }

    fn delete(&self, project: &ProjectName, id: &WorkItemId) -> Result<(), Self::Error> {
        let mut state = self.lock();
        let items = state.items.entry(project.clone()).or_default();
        let before = items.len();
        items.retain(|item| item.id.as_item() != Some(id));
        assert!(before > items.len(), "delete of unknown id {id:?}");
        Ok(())
    }
}

fn render_tags(tags: &pwf_models::pending_work::Tags) -> String {
    format!(
        "[{}]",
        tags.iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    )
}

impl AppRecordStore<ProjectNote> for InMemoryStore {
    type Error = InMemoryStoreError;

    fn get(&self, project: &ProjectName, id: &NoteId) -> Result<Option<ProjectNote>, Self::Error> {
        Ok(self
            .lock()
            .project_notes
            .get(project)
            .and_then(|notes| notes.iter().find(|note| note.id == *id).cloned()))
    }

    fn list(&self, project: &ProjectName) -> Result<Vec<ProjectNote>, Self::Error> {
        if self
            .lock()
            .project_note_failures
            .contains(&ProjectNoteFailure::List)
        {
            return Err(InMemoryStoreError::Injected {
                operation: "project-note-list",
            });
        }
        Ok(self
            .lock()
            .project_notes
            .get(project)
            .cloned()
            .unwrap_or_default())
    }

    fn insert(
        &self,
        project: &ProjectName,
        new: NewProjectNote,
    ) -> Result<ProjectNote, Self::Error> {
        let record = ProjectNote {
            id: new.id,
            topic: new.topic,
        };
        let mut state = self.lock();
        state
            .project_note_creations
            .entry(project.clone())
            .or_default()
            .push(new.created);
        state
            .project_notes
            .entry(project.clone())
            .or_default()
            .push(record.clone());
        Ok(record)
    }

    fn update(
        &self,
        project: &ProjectName,
        id: &NoteId,
        patch: ProjectNotePatch,
    ) -> Result<(), Self::Error> {
        let mut state = self.lock();
        let note = state
            .project_notes
            .entry(project.clone())
            .or_default()
            .iter_mut()
            .find(|note| note.id == *id)
            .expect("update of unknown project note");
        note.topic = patch.topic;
        Ok(())
    }

    fn delete(&self, project: &ProjectName, id: &NoteId) -> Result<(), Self::Error> {
        if self
            .lock()
            .project_note_failures
            .contains(&ProjectNoteFailure::Delete)
        {
            return Err(InMemoryStoreError::Injected {
                operation: "project-note-delete",
            });
        }
        let mut state = self.lock();
        let notes = state.project_notes.entry(project.clone()).or_default();
        let count_before = notes.len();
        notes.retain(|note| note.id != *id);
        assert!(count_before > notes.len(), "delete of unknown project note");
        Ok(())
    }
}

impl ProjectNoteStore for InMemoryStore {
    fn note_exists(
        &self,
        project: &ProjectName,
        id: &NoteId,
    ) -> Result<bool, <Self as AppRecordStore<ProjectNote>>::Error> {
        Ok(self
            .lock()
            .project_notes
            .get(project)
            .is_some_and(|notes| notes.iter().any(|note| note.id == *id)))
    }

    fn read_note_markdown(
        &self,
        locator: &str,
    ) -> Result<String, <Self as AppRecordStore<ProjectNote>>::Error> {
        if self
            .lock()
            .project_note_failures
            .contains(&ProjectNoteFailure::Read)
        {
            return Err(InMemoryStoreError::Injected {
                operation: "project-note-read",
            });
        }
        self.lock()
            .items
            .values()
            .flatten()
            .find(|item| item.locator == locator)
            .map(|item| item.source.clone())
            .ok_or_else(|| InMemoryStoreError::PendingWorkNoteMarkdownMissing {
                locator: locator.to_string(),
            })
    }
}

impl AppRecordStore<IndexEntry> for InMemoryStore {
    type Error = Infallible;

    fn get(
        &self,
        project: &ProjectName,
        id: &WorkItemId,
    ) -> Result<Option<IndexEntry>, Self::Error> {
        Ok(self
            .lock()
            .entries
            .get(project)
            .and_then(|entries| entries.iter().find(|entry| entry.id == *id).cloned()))
    }

    fn list(&self, project: &ProjectName) -> Result<Vec<IndexEntry>, Self::Error> {
        Ok(self
            .lock()
            .entries
            .get(project)
            .cloned()
            .unwrap_or_default())
    }

    fn insert(&self, project: &ProjectName, new: IndexEntry) -> Result<IndexEntry, Self::Error> {
        self.upsert(project, new.clone());
        Ok(new)
    }

    fn update(
        &self,
        project: &ProjectName,
        _id: &WorkItemId,
        patch: IndexEntry,
    ) -> Result<(), Self::Error> {
        self.upsert(project, patch);
        Ok(())
    }

    fn delete(&self, project: &ProjectName, id: &WorkItemId) -> Result<(), Self::Error> {
        let mut state = self.lock();
        state
            .entries
            .entry(project.clone())
            .or_default()
            .retain(|entry| entry.id != *id);
        Ok(())
    }
}

impl InMemoryStore {
    /// Replaces or appends an entry and creates its section when needed.
    fn upsert(&self, project: &ProjectName, entry: IndexEntry) {
        let mut state = self.lock();
        if !entry.section.is_empty() {
            let sections = state.sections.entry(project.clone()).or_default();
            if !sections.iter().any(|label| label == &entry.section) {
                sections.push(entry.section.clone());
            }
        }
        let entries = state.entries.entry(project.clone()).or_default();
        match entries.iter_mut().find(|existing| existing.id == entry.id) {
            Some(existing) => *existing = entry,
            None => entries.push(entry),
        }
    }
}

impl AppRecordStore<IndexSection> for InMemoryStore {
    type Error = InMemoryStoreError;

    fn get(
        &self,
        project: &ProjectName,
        label: &String,
    ) -> Result<Option<IndexSection>, Self::Error> {
        Ok(<Self as AppRecordStore<IndexSection>>::list(self, project)?
            .into_iter()
            .find(|section| section.label == *label))
    }

    fn list(&self, project: &ProjectName) -> Result<Vec<IndexSection>, Self::Error> {
        Ok(self
            .lock()
            .sections
            .get(project)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|label| IndexSection { label })
            .collect())
    }

    /// Rejects direct section creation, matching the production adapter.
    fn insert(
        &self,
        _project: &ProjectName,
        _new: IndexSection,
    ) -> Result<IndexSection, Self::Error> {
        Err(InMemoryStoreError::IndexSectionWriteUnsupported { op: "insert" })
    }

    /// Renames a section and updates entries that referenced its old label.
    fn update(
        &self,
        project: &ProjectName,
        label: &String,
        patch: IndexSection,
    ) -> Result<(), Self::Error> {
        let mut state = self.lock();
        if let Some(sections) = state.sections.get_mut(project) {
            for existing in sections.iter_mut() {
                if existing == label {
                    *existing = patch.label.clone();
                }
            }
        }
        if let Some(entries) = state.entries.get_mut(project) {
            for entry in entries.iter_mut() {
                if entry.section == *label {
                    entry.section = patch.label.clone();
                }
            }
        }
        Ok(())
    }

    /// Rejects section deletion, matching the production adapter.
    fn delete(&self, _project: &ProjectName, _label: &String) -> Result<(), Self::Error> {
        Err(InMemoryStoreError::IndexSectionWriteUnsupported { op: "delete" })
    }
}
