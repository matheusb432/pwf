use std::{
    collections::BTreeMap,
    convert::Infallible,
    fmt::Write as _,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
    time::SystemTime,
};

use pwf_domain::{
    handoff::HandoffStatus,
    pending_work::{ProjectName, WorkItemId, WorkItemStatus},
};

use crate::ports::{
    AppRecordStore, HandoffDocument, HandoffDocumentIdentifier, HandoffDocumentScopePresence,
    HandoffDocumentStore, HandoffLedger, HandoffLedgerIdentifier, HandoffLedgerWrite,
    HandoffLocation, HandoffPatch, HandoffScope, IndexEntry, IndexSection, ItemPatch,
    Materialization, NewHandoffDocument, NewItem, PendingWorkItem, RecordId,
};

#[derive(Debug, Default)]
struct InMemoryState {
    items: BTreeMap<ProjectName, Vec<PendingWorkItem>>,
    entries: BTreeMap<ProjectName, Vec<IndexEntry>>,
    sections: BTreeMap<ProjectName, Vec<String>>,
    prefixes: BTreeMap<ProjectName, String>,
    handoff_documents: BTreeMap<HandoffScope, Vec<HandoffDocument>>,
    handoff_scope_presences: BTreeMap<HandoffScope, HandoffDocumentScopePresence>,
    handoff_ledgers: BTreeMap<HandoffScope, HandoffLedger>,
    failure_points: Vec<FailurePoint>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FailurePoint {
    DocumentDelete,
    DocumentInsert,
    DocumentRestoreDelete,
    DocumentRestoreMove,
    DocumentUpdate,
    LedgerInsert,
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
    #[error("handoff document already exists: {file_name}")]
    HandoffDocumentExists { file_name: String },
    #[error("injected in-memory store failure: {operation}")]
    Injected { operation: &'static str },
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

    /// Stages parsed handoff documents for application tests.
    pub fn with_handoff_documents(
        self,
        scope: HandoffScope,
        documents: Vec<HandoffDocument>,
    ) -> Self {
        self.lock().handoff_documents.insert(scope, documents);
        self
    }

    /// Returns staged handoff documents in insertion order.
    pub fn handoff_documents(&self, scope: &HandoffScope) -> Vec<HandoffDocument> {
        self.lock()
            .handoff_documents
            .get(scope)
            .cloned()
            .unwrap_or_default()
    }

    /// Returns the current derived handoff ledger.
    pub fn handoff_ledger(&self, scope: &HandoffScope) -> Option<HandoffLedger> {
        self.lock().handoff_ledgers.get(scope).cloned()
    }

    /// Stages repository and active-directory presence for application tests.
    pub fn with_handoff_scope_presence(
        self,
        scope: HandoffScope,
        presence: HandoffDocumentScopePresence,
    ) -> Self {
        self.lock().handoff_scope_presences.insert(scope, presence);
        self
    }

    pub(crate) fn with_failure(self, failure_point: FailurePoint) -> Self {
        self.lock().failure_points.push(failure_point);
        self
    }

    pub(crate) fn with_failures(
        self,
        failure_points: impl IntoIterator<Item = FailurePoint>,
    ) -> Self {
        self.lock().failure_points.extend(failure_points);
        self
    }

    fn lock(&self) -> MutexGuard<'_, InMemoryState> {
        self.state.lock().expect("in-memory store lock poisoned")
    }
}

fn project_name(project: &str) -> ProjectName {
    ProjectName::try_new(project).expect("test project name is non-empty")
}

impl AppRecordStore<PendingWorkItem> for InMemoryStore {
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
            tags: new.tags.map(|tags| tags.frontmatter_value()),
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

impl AppRecordStore<HandoffDocument> for InMemoryStore {
    type Error = InMemoryStoreError;

    fn get(
        &self,
        scope: &HandoffScope,
        identifier: &HandoffDocumentIdentifier,
    ) -> Result<Option<HandoffDocument>, Self::Error> {
        Ok(self
            .lock()
            .handoff_documents
            .get(scope)
            .and_then(|documents| {
                documents
                    .iter()
                    .find(|document| document.identifier == *identifier)
                    .cloned()
            }))
    }

    fn list(&self, scope: &HandoffScope) -> Result<Vec<HandoffDocument>, Self::Error> {
        Ok(self
            .lock()
            .handoff_documents
            .get(scope)
            .cloned()
            .unwrap_or_default())
    }

    fn insert(
        &self,
        scope: &HandoffScope,
        new: NewHandoffDocument,
    ) -> Result<HandoffDocument, Self::Error> {
        if self
            .lock()
            .failure_points
            .contains(&FailurePoint::DocumentInsert)
        {
            return Err(InMemoryStoreError::Injected {
                operation: "handoff-document-insert",
            });
        }
        let identifier = HandoffDocumentIdentifier {
            file_name: new.file_name.clone(),
            location: HandoffLocation::Active,
        };
        let mut state = self.lock();
        let documents = state.handoff_documents.entry(scope.clone()).or_default();
        if documents
            .iter()
            .any(|document| document.identifier == identifier)
        {
            return Err(InMemoryStoreError::HandoffDocumentExists {
                file_name: new.file_name,
            });
        }
        let locator = handoff_document_path(scope, &identifier);
        let mut record = HandoffDocument {
            identifier,
            location: HandoffLocation::Active,
            project: Some(new.project),
            title: new.title,
            status: Some(HandoffStatus::Active),
            created: Some(new.created),
            completed: None,
            pending_work_identifier_raw: new
                .pending_work_identifier
                .map(|identifier| identifier.to_string()),
            goals_completed: 0,
            goals_total: 0,
            body: new.body,
            source: String::new(),
            locator,
            modified_timestamp: SystemTime::UNIX_EPOCH,
        };
        refresh_handoff_document(&mut record);
        documents.push(record.clone());
        Ok(record)
    }

    fn update(
        &self,
        scope: &HandoffScope,
        identifier: &HandoffDocumentIdentifier,
        patch: HandoffPatch,
    ) -> Result<(), Self::Error> {
        if self
            .lock()
            .failure_points
            .contains(&FailurePoint::DocumentUpdate)
        {
            return Err(InMemoryStoreError::Injected {
                operation: "handoff-document-update",
            });
        }
        let mut state = self.lock();
        let document = state
            .handoff_documents
            .entry(scope.clone())
            .or_default()
            .iter_mut()
            .find(|document| document.identifier == *identifier)
            .expect("update of unknown handoff document");
        if let Some(status) = patch.status {
            document.status = Some(status);
        }
        if let Some(completed) = patch.completed {
            document.completed = completed;
        }
        if let Some(pending_work_identifier) = patch.pending_work_identifier {
            document.pending_work_identifier_raw = Some(pending_work_identifier.to_string());
        }
        if let Some(body) = patch.body {
            document.body = body;
        }
        if let Some(location) = patch.location {
            document.location = location;
            document.identifier.location = location;
            document.locator = handoff_document_path(scope, &document.identifier);
        }
        refresh_handoff_document(document);
        Ok(())
    }

    fn delete(
        &self,
        scope: &HandoffScope,
        identifier: &HandoffDocumentIdentifier,
    ) -> Result<(), Self::Error> {
        if self
            .lock()
            .failure_points
            .contains(&FailurePoint::DocumentDelete)
        {
            return Err(InMemoryStoreError::Injected {
                operation: "handoff-document-delete",
            });
        }
        self.lock()
            .handoff_documents
            .entry(scope.clone())
            .or_default()
            .retain(|document| document.identifier != *identifier);
        Ok(())
    }
}

impl HandoffDocumentStore for InMemoryStore {
    fn scope_presence(
        &self,
        scope: &HandoffScope,
    ) -> Result<HandoffDocumentScopePresence, <Self as AppRecordStore<HandoffDocument>>::Error>
    {
        Ok(self
            .lock()
            .handoff_scope_presences
            .get(scope)
            .copied()
            .unwrap_or(HandoffDocumentScopePresence::Present))
    }

    fn document_exists(
        &self,
        scope: &HandoffScope,
        identifier: &HandoffDocumentIdentifier,
    ) -> Result<bool, <Self as AppRecordStore<HandoffDocument>>::Error> {
        Ok(self
            .lock()
            .handoff_documents
            .get(scope)
            .is_some_and(|documents| {
                documents
                    .iter()
                    .any(|document| document.identifier == *identifier)
            }))
    }

    fn list_location(
        &self,
        scope: &HandoffScope,
        location: HandoffLocation,
    ) -> Result<Vec<HandoffDocument>, <Self as AppRecordStore<HandoffDocument>>::Error> {
        Ok(self
            .lock()
            .handoff_documents
            .get(scope)
            .into_iter()
            .flatten()
            .filter(|document| document.location == location)
            .cloned()
            .collect())
    }

    fn restore_document_after_move(
        &self,
        scope: &HandoffScope,
        snapshot: &HandoffDocument,
    ) -> Result<(), <Self as AppRecordStore<HandoffDocument>>::Error> {
        if self
            .lock()
            .failure_points
            .contains(&FailurePoint::DocumentRestoreMove)
        {
            return Err(InMemoryStoreError::Injected {
                operation: "handoff-document-restore-move",
            });
        }
        let mut state = self.lock();
        let documents = state.handoff_documents.entry(scope.clone()).or_default();
        restore_in_memory_document(documents, snapshot);
        let moved_location = match snapshot.identifier.location {
            HandoffLocation::Active => HandoffLocation::Archived,
            HandoffLocation::Archived => HandoffLocation::Active,
        };
        documents.retain(|document| {
            document.identifier.file_name != snapshot.identifier.file_name
                || document.identifier.location != moved_location
        });
        Ok(())
    }

    fn restore_document_after_delete(
        &self,
        scope: &HandoffScope,
        snapshot: &HandoffDocument,
    ) -> Result<(), <Self as AppRecordStore<HandoffDocument>>::Error> {
        if self
            .lock()
            .failure_points
            .contains(&FailurePoint::DocumentRestoreDelete)
        {
            return Err(InMemoryStoreError::Injected {
                operation: "handoff-document-restore-delete",
            });
        }
        let mut state = self.lock();
        restore_in_memory_document(
            state.handoff_documents.entry(scope.clone()).or_default(),
            snapshot,
        );
        Ok(())
    }
}

fn restore_in_memory_document(documents: &mut Vec<HandoffDocument>, snapshot: &HandoffDocument) {
    if let Some(document) = documents
        .iter_mut()
        .find(|document| document.identifier == snapshot.identifier)
    {
        *document = snapshot.clone();
    } else {
        documents.push(snapshot.clone());
    }
}

impl AppRecordStore<HandoffLedger> for InMemoryStore {
    type Error = InMemoryStoreError;

    fn get(
        &self,
        scope: &HandoffScope,
        _identifier: &HandoffLedgerIdentifier,
    ) -> Result<Option<HandoffLedger>, Self::Error> {
        Ok(self.lock().handoff_ledgers.get(scope).cloned())
    }

    fn list(&self, scope: &HandoffScope) -> Result<Vec<HandoffLedger>, Self::Error> {
        Ok(self
            .lock()
            .handoff_ledgers
            .get(scope)
            .cloned()
            .into_iter()
            .collect())
    }

    fn insert(
        &self,
        scope: &HandoffScope,
        new: HandoffLedgerWrite,
    ) -> Result<HandoffLedger, Self::Error> {
        if self
            .lock()
            .failure_points
            .contains(&FailurePoint::LedgerInsert)
        {
            return Err(InMemoryStoreError::Injected {
                operation: "handoff-ledger-insert",
            });
        }
        let ledger = HandoffLedger {
            source: render_handoff_ledger(&new),
            locator: scope
                .repository_root
                .join("docs")
                .join("handoffs")
                .join("LEDGER.md"),
        };
        self.lock()
            .handoff_ledgers
            .insert(scope.clone(), ledger.clone());
        Ok(ledger)
    }

    fn update(
        &self,
        scope: &HandoffScope,
        _identifier: &HandoffLedgerIdentifier,
        patch: HandoffLedgerWrite,
    ) -> Result<(), Self::Error> {
        let _ = <Self as AppRecordStore<HandoffLedger>>::insert(self, scope, patch)?;
        Ok(())
    }

    fn delete(
        &self,
        scope: &HandoffScope,
        _identifier: &HandoffLedgerIdentifier,
    ) -> Result<(), Self::Error> {
        self.lock().handoff_ledgers.remove(scope);
        Ok(())
    }
}

fn handoff_document_path(scope: &HandoffScope, identifier: &HandoffDocumentIdentifier) -> PathBuf {
    let directory = scope.repository_root.join("docs").join("handoffs");
    match identifier.location {
        HandoffLocation::Active => directory.join(&identifier.file_name),
        HandoffLocation::Archived => directory.join("archived").join(&identifier.file_name),
    }
}

fn refresh_handoff_document(document: &mut HandoffDocument) {
    document.title = handoff_title(
        &document.body,
        document.identifier.file_name.trim_end_matches(".md"),
    );
    (document.goals_completed, document.goals_total) = goal_counts(&document.body);
    document.source = render_handoff_document(document);
    document.modified_timestamp = SystemTime::now();
}

fn handoff_title(body: &str, fallback: &str) -> String {
    body.lines()
        .filter_map(|line| line.strip_prefix('#'))
        .find_map(|heading| {
            heading
                .starts_with(char::is_whitespace)
                .then(|| heading.trim())
                .filter(|heading| !heading.is_empty())
        })
        .map_or_else(|| fallback.to_string(), str::to_string)
}

fn render_handoff_document(document: &HandoffDocument) -> String {
    let mut source = String::from("---\n");
    if let Some(status) = document.status {
        let _ = writeln!(source, "status: {status}");
    }
    if let Some(project) = &document.project {
        let _ = writeln!(source, "project: {project}");
    }
    if let Some(created) = &document.created {
        let _ = writeln!(source, "created: {}", created.as_str());
    }
    if let Some(completed) = &document.completed {
        let _ = writeln!(source, "completed: {}", completed.as_str());
    }
    if let Some(identifier) = &document.pending_work_identifier_raw {
        let _ = writeln!(source, "pw: {identifier}");
    }
    source.push_str("---\n");
    source.push_str(&document.body);
    source
}

fn render_handoff_ledger(write: &HandoffLedgerWrite) -> String {
    let mut source = String::from(
        "# Handoff ledger — active only\n\nOnly handoffs with status: active are listed.\n\n| ID | Handoff | Goals | Created |\n| --- | --- | --- | --- |\n",
    );
    for row in &write.rows {
        let _ = writeln!(
            source,
            "| {} | [{}]({}) | {}/{} | {} |",
            row.pending_work_identifier,
            row.title,
            row.file_name,
            row.goals_completed,
            row.goals_total,
            row.created
        );
    }
    source
}

fn goal_counts(body: &str) -> (usize, usize) {
    body.lines().fold((0, 0), |(completed, total), line| {
        let trimmed = line.trim_start();
        if trimmed.starts_with("- [x]") || trimmed.starts_with("- [X]") {
            (completed + 1, total + 1)
        } else if trimmed.starts_with("- [ ]") {
            (completed, total + 1)
        } else {
            (completed, total)
        }
    })
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

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::SystemTime};

    use pwf_domain::{
        handoff::HandoffStatus,
        pending_work::{ProjectName, Timestamp, WorkItemId},
    };

    use super::{AppRecordStore, InMemoryStore};
    use crate::{
        HandoffDocument, HandoffDocumentIdentifier, HandoffLocation, HandoffPatch, HandoffScope,
        NewHandoffDocument,
    };

    #[test]
    fn handoff_update_keeps_typed_fields_raw_source_location_and_timestamp_consistent() {
        let scope = HandoffScope {
            repository_root: PathBuf::from("/repo/test-project"),
        };
        let identifier = HandoffDocumentIdentifier {
            file_name: "2026-01-01-managed-flow.md".to_string(),
            location: HandoffLocation::Active,
        };
        let store = InMemoryStore::default();
        <InMemoryStore as AppRecordStore<HandoffDocument>>::insert(
            &store,
            &scope,
            NewHandoffDocument {
                file_name: identifier.file_name.clone(),
                project: ProjectName::try_new("test-project").unwrap(),
                title: "Managed Flow".to_string(),
                created: Timestamp::new("2026-01-01"),
                body: "\n# Managed Flow\n\n- [ ] original\n".to_string(),
                pending_work_identifier: None,
            },
        )
        .unwrap();
        store.lock().handoff_documents.get_mut(&scope).unwrap()[0].modified_timestamp =
            SystemTime::UNIX_EPOCH;

        <InMemoryStore as AppRecordStore<HandoffDocument>>::update(
            &store,
            &scope,
            &identifier,
            HandoffPatch {
                location: Some(HandoffLocation::Archived),
                status: Some(HandoffStatus::Done),
                completed: Some(Some(Timestamp::new("2026-01-02"))),
                pending_work_identifier: Some(WorkItemId::try_new("TST-0042").unwrap()),
                body: Some("\n# Renamed Flow\n\n- [x] first\n- [ ] second\n".to_string()),
            },
        )
        .unwrap();

        let document = store.handoff_documents(&scope).pop().unwrap();
        assert_eq!(document.status, Some(HandoffStatus::Done));
        assert_eq!(
            document.completed.as_ref().map(Timestamp::as_str),
            Some("2026-01-02")
        );
        assert_eq!(
            document.pending_work_identifier_raw.as_deref(),
            Some("TST-0042")
        );
        assert_eq!(document.goals_completed, 1);
        assert_eq!(document.goals_total, 2);
        assert_eq!(document.title, "Renamed Flow");
        assert_eq!(
            document.body,
            "\n# Renamed Flow\n\n- [x] first\n- [ ] second\n"
        );
        assert_eq!(document.location, HandoffLocation::Archived);
        assert_eq!(document.identifier.location, HandoffLocation::Archived);
        assert_eq!(
            document.locator,
            scope
                .repository_root
                .join("docs/handoffs/archived/2026-01-01-managed-flow.md")
        );
        assert_eq!(
            document.source,
            "---\nstatus: done\nproject: test-project\ncreated: 2026-01-01\ncompleted: 2026-01-02\npw: TST-0042\n---\n\n# Renamed Flow\n\n- [x] first\n- [ ] second\n"
        );
        assert!(document.modified_timestamp > SystemTime::UNIX_EPOCH);
    }
}
