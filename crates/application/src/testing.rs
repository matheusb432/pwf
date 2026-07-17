use std::{
    collections::BTreeMap,
    convert::Infallible,
    sync::{Arc, Mutex, MutexGuard},
};

use pwf_domain::pending_work::{ProjectName, WorkItemId, WorkItemStatus};

use crate::ports::{
    AppDbStore, IndexEntry, IndexSection, ItemPatch, Materialization, NewItem, PendingWorkItem,
    RecordId,
};

#[derive(Debug, Default)]
struct InMemoryState {
    items: BTreeMap<ProjectName, Vec<PendingWorkItem>>,
    entries: BTreeMap<ProjectName, Vec<IndexEntry>>,
    sections: BTreeMap<ProjectName, Vec<String>>,
    prefixes: BTreeMap<ProjectName, String>,
}

/// Provides a thread-safe [`AppDbStore`] test double for all pending-work record kinds.
///
/// Clones share one `Arc<Mutex<InMemoryState>>` and therefore observe the same writes.
#[derive(Debug, Clone, Default)]
pub struct InMemoryStore {
    state: Arc<Mutex<InMemoryState>>,
}

/// Reports unsupported direct [`IndexSection`] writes.
///
/// Sections are created implicitly by an [`IndexEntry`] upsert, matching the production store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InMemoryStoreError {
    #[error("index sections are managed implicitly and cannot be written directly (op: {op})")]
    IndexSectionWriteUnsupported { op: &'static str },
}

impl InMemoryStore {
    pub fn with_project(self, project: &str, items: Vec<PendingWorkItem>) -> Self {
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

    pub fn items(&self, project: &str) -> Vec<PendingWorkItem> {
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

    fn lock(&self) -> MutexGuard<'_, InMemoryState> {
        self.state.lock().expect("in-memory store lock poisoned")
    }
}

fn project_name(project: &str) -> ProjectName {
    ProjectName::try_new(project).expect("test project name is non-empty")
}

impl AppDbStore<PendingWorkItem> for InMemoryStore {
    type Error = Infallible;

    fn get(
        &self,
        project: &ProjectName,
        id: &WorkItemId,
    ) -> Result<Option<PendingWorkItem>, Self::Error> {
        Ok(self.lock().items.get(project).and_then(|items| {
            items
                .iter()
                .find(|item| item.id.as_item() == Some(id))
                .cloned()
        }))
    }

    fn list(&self, project: &ProjectName) -> Result<Vec<PendingWorkItem>, Self::Error> {
        Ok(self.lock().items.get(project).cloned().unwrap_or_default())
    }

    /// Allocates the next `<prefix>-NNNN` id and materializes an active record.
    fn insert(&self, project: &ProjectName, new: NewItem) -> Result<PendingWorkItem, Self::Error> {
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
        let record = PendingWorkItem {
            id: RecordId::Item(id),
            title: new.title.clone().unwrap_or_else(|| "n/a".to_string()),
            status: WorkItemStatus::Active,
            created: Some(new.created),
            completed: None,
            commits: None,
            tags: None,
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
            record.title = title;
        }
        if let Some(prereq) = patch.prereq {
            record.prereq = prereq;
        }
        if let Some(effort) = patch.effort {
            record.effort = Some(effort.to_string());
        }
        if let Some(tags) = patch.tags {
            record.tags = tags.map(|tags| tags.frontmatter_value());
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

impl AppDbStore<IndexEntry> for InMemoryStore {
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

impl AppDbStore<IndexSection> for InMemoryStore {
    type Error = InMemoryStoreError;

    fn get(
        &self,
        project: &ProjectName,
        label: &String,
    ) -> Result<Option<IndexSection>, Self::Error> {
        Ok(<Self as AppDbStore<IndexSection>>::list(self, project)?
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
