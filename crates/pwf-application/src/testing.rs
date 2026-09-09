mod database;

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, MutexGuard},
};

pub(crate) use database::{MIGRATOR, insert_project};
use pwf_models::{
    AppDate,
    note::{NoteId, NoteTitle, ProjectNote},
    project::{
        Project, ProjectId, ProjectName, ProjectSource, ProjectSourceKind, ProjectSourceValue,
        ProjectTasks, ProjectTasksKind, ProjectTasksPath,
    },
    revision::ContentRevision,
    task::{BlockedBy, TaskId, TaskStatus, TaskTags, TaskTimestamp},
};
use pwf_wire::task::RawTaskTags;

use crate::ports::{
    clock::Clock,
    project_note::{NewProjectNote, ProjectNotePatch, ProjectNotes},
    task_vault::{
        IndexEntry, Materialization, NewTask, NullablePatch, StoredBlockedBy, TaskMutationError,
        TaskPatch, TaskRecord, TaskRevisionState, TaskVault, TaskWrite, TaskWriteSet,
    },
};

pub(crate) fn project_note(number: u32, title: impl AsRef<str>) -> ProjectNote {
    ProjectNote {
        id: NoteId::try_new(format!("FOO-NOTE-{number:04}")).unwrap(),
        title: NoteTitle::try_new(title.as_ref()).unwrap(),
        verified: None,
    }
}

#[derive(Clone)]
pub(crate) struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> Result<TaskTimestamp, pwf_models::task::TaskTimestampError> {
        "2026-07-26T12:34:56Z".parse()
    }
}

#[derive(Debug, Default)]
struct InMemoryState {
    tasks: BTreeMap<ProjectName, Vec<TaskRecord>>,
    entries: BTreeMap<ProjectName, Vec<IndexEntry>>,
    project_ids: BTreeMap<ProjectName, ProjectId>,
    project_notes: BTreeMap<ProjectName, Vec<ProjectNote>>,
    project_note_creations: BTreeMap<ProjectName, Vec<AppDate>>,
    project_note_patches: BTreeMap<ProjectName, Vec<ProjectNotePatch>>,
    failures: Vec<InMemoryStoreFailure>,
    next_task_revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InMemoryStoreFailure {
    DeleteProjectNote,
    ListTasks,
    ListProjectNotes,
    ReadTask,
    ReadTaskMarkdown,
}

/// Provides thread-safe in-memory persistence for application tests.
///
/// Clones share one `Arc<Mutex<InMemoryState>>` and therefore observe the same writes.
#[derive(Debug, Clone, Default)]
pub struct InMemoryStore {
    state: Arc<Mutex<InMemoryState>>,
}

/// Reports rejected writes in the application test store.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InMemoryStoreError {
    #[error("injected in-memory store failure: {operation}")]
    Injected { operation: &'static str },
    #[error("task note Markdown is not staged: {locator}")]
    TaskNoteMarkdownMissing { locator: String },
    #[error("task {id} already exists")]
    TaskAlreadyExists { id: TaskId },
    #[error("task {id} does not exist")]
    TaskNotFound { id: TaskId },
}

impl InMemoryStore {
    pub fn with_project(self, project: &str, tasks: Vec<TaskRecord>) -> Self {
        self.lock().tasks.insert(project_name(project), tasks);
        self
    }

    /// Registers the project ID used by task insertion.
    pub fn with_project_id(self, project: &str, project_id: &str) -> Self {
        self.lock()
            .project_ids
            .insert(project_name(project), project_id.parse().unwrap());
        self
    }

    pub fn tasks(&self, project: &str) -> Vec<TaskRecord> {
        self.lock()
            .tasks
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

    pub(crate) fn externally_edit_task(&self, project: &Project, id: &TaskId) {
        let mut state = self.lock();
        let revision = next_task_revision(&mut state);
        let record = state
            .tasks
            .entry(project.title.clone())
            .or_default()
            .iter_mut()
            .find(|task| task.id == *id)
            .unwrap();
        record.source.push_str("\nexternal edit\n");
        record.body.push_str("\nexternal edit\n");
        record.revision = revision;
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

    pub fn project_note_creations(&self, project: &str) -> Vec<AppDate> {
        self.lock()
            .project_note_creations
            .get(&project_name(project))
            .cloned()
            .unwrap_or_default()
    }

    pub fn project_note_patches(&self, project: &str) -> Vec<ProjectNotePatch> {
        self.lock()
            .project_note_patches
            .get(&project_name(project))
            .cloned()
            .unwrap_or_default()
    }

    pub(crate) fn with_failure(self, failure: InMemoryStoreFailure) -> Self {
        self.lock().failures.push(failure);
        self
    }

    fn lock(&self) -> MutexGuard<'_, InMemoryState> {
        self.state.lock().unwrap()
    }
}

fn project_name(project: &str) -> ProjectName {
    ProjectName::try_new(project).unwrap()
}

pub(crate) fn project(project_id: &str, title: &str) -> Project {
    Project {
        obsidian_vault: None,
        id: project_id.parse().unwrap(),
        title: project_name(title),
        source: Some(ProjectSource::new(
            ProjectSourceKind::Directory,
            ProjectSourceValue::try_new(format!("/work/{title}")).unwrap(),
        )),
        tasks: ProjectTasks::new(
            ProjectTasksKind::Directory,
            ProjectTasksPath::try_new(format!("/tasks/{title}")).unwrap(),
        ),
        created_at: "2026-07-25T00:00:00.000Z".parse().unwrap(),
        is_paused: false,
    }
}

pub(crate) fn app_date(raw: impl AsRef<str>) -> AppDate {
    raw.as_ref().parse().unwrap()
}

pub(crate) fn task_timestamp(raw: impl AsRef<str>) -> TaskTimestamp {
    raw.as_ref().parse().unwrap()
}

pub(crate) fn task_record(id: &str) -> TaskRecord {
    TaskRecord {
        id: TaskId::try_new(id).unwrap(),
        title: "tray gui".to_string(),
        status: TaskStatus::Active,
        created_at: Some(task_timestamp("2026-01-01T00:00:00Z")),
        completed_at: None,
        commits: None,
        tags: None,
        effort: None,
        priority: None,
        blocked_by: crate::ports::task_vault::StoredBlockedBy::Absent,
        section: None,
        body: "\nbody\n".to_string(),
        source: "body".to_string(),
        locator: pwf_wire::task::TaskNotePath::new(format!("/mem/foo-bar/{id}.md").into()),
        placement: None,
        materialization: Materialization::NoteFile,
        revision: ContentRevision::try_new("0".repeat(64)).unwrap(),
    }
}

pub(crate) fn blocked_by(ids: &[&str]) -> BlockedBy {
    BlockedBy::try_new(ids.iter().map(|id| id.parse().unwrap())).unwrap()
}

pub(crate) fn stored_blocked_by(ids: &[&str]) -> StoredBlockedBy {
    StoredBlockedBy::Valid(blocked_by(ids))
}

pub(crate) const FOO_0001_SOURCE: &str = "---\nid: FOO-0001\nstatus: active\ntitle: do the thing\nproject: foo\ncreated_at: 2026-06-20T00:00:00Z\n---\n\n## Goals\n- do the thing\n";

pub(crate) fn staged_task() -> (InMemoryStore, Vec<Project>) {
    let record = TaskRecord {
        title: "do the thing".to_string(),
        created_at: Some(task_timestamp("2026-06-20T00:00:00Z")),
        body: "\n## Goals\n- do the thing\n".to_string(),
        source: FOO_0001_SOURCE.to_string(),
        locator: pwf_wire::task::TaskNotePath::new("/notes/foo/FOO-0001.md".into()),
        ..task_record("FOO-0001")
    };
    (
        InMemoryStore::default().with_project("foo", vec![record]),
        vec![project("FOO", "foo")],
    )
}

pub(crate) fn staged_missing_task() -> (InMemoryStore, Vec<Project>) {
    let expected = "/notes/foo/FOO-0002.md".to_string();
    let record = TaskRecord {
        title: "ghost".to_string(),
        created_at: None,
        body: String::new(),
        source: String::new(),
        locator: pwf_wire::task::TaskNotePath::new(expected.clone().into()),
        materialization: Materialization::MissingNote {
            expected: pwf_wire::task::TaskNotePath::new(expected.into()),
        },
        ..task_record("FOO-0002")
    };
    (
        InMemoryStore::default().with_project("foo", vec![record]),
        vec![project("FOO", "foo")],
    )
}

impl TaskVault for InMemoryStore {
    type Error = InMemoryStoreError;

    fn task_deletion(
        &self,
        project: &Project,
    ) -> Result<pwf_wire::confirmation::TaskDeletion, Self::Error> {
        Ok(project.obsidian_vault.as_ref().map_or(
            pwf_wire::confirmation::TaskDeletion::HardDelete,
            |vault| pwf_wire::confirmation::TaskDeletion::MoveToTrash {
                obsidian_vault: vault.as_ref().into(),
            },
        ))
    }

    fn get_task(&self, project: &Project, id: &TaskId) -> Result<Option<TaskRecord>, Self::Error> {
        if self
            .lock()
            .failures
            .contains(&InMemoryStoreFailure::ReadTask)
        {
            return Err(InMemoryStoreError::Injected {
                operation: "task-read",
            });
        }
        Ok(self
            .lock()
            .tasks
            .get(&project.title)
            .and_then(|tasks| tasks.iter().find(|task| task.id == *id).cloned()))
    }

    fn list_tasks(&self, project: &Project) -> Result<Vec<TaskRecord>, Self::Error> {
        if self
            .lock()
            .failures
            .contains(&InMemoryStoreFailure::ListTasks)
        {
            return Err(InMemoryStoreError::Injected {
                operation: "task-list",
            });
        }
        Ok(self
            .lock()
            .tasks
            .get(&project.title)
            .cloned()
            .unwrap_or_default())
    }

    fn next_task_id(&self, project: &Project) -> Result<TaskId, Self::Error> {
        let state = self.lock();
        let project_id = state.project_ids.get(&project.title).cloned().unwrap();
        let next = state
            .tasks
            .get(&project.title)
            .into_iter()
            .flatten()
            .map(|task| task.id.number())
            .max()
            .unwrap_or(0)
            + 1;
        Ok(TaskId::try_new(format!("{project_id}-{next:04}")).unwrap())
    }

    /// Materializes an active record at the exact prevalidated task ID.
    fn insert_task(
        &self,
        project: &Project,
        id: &TaskId,
        new: NewTask,
    ) -> Result<TaskRecord, Self::Error> {
        let mut state = self.lock();
        let revision = next_task_revision(&mut state);
        let tasks = state.tasks.entry(project.title.clone()).or_default();
        if tasks.iter().any(|task| task.id == *id) {
            return Err(InMemoryStoreError::TaskAlreadyExists { id: id.clone() });
        }
        let locator = pwf_wire::task::TaskNotePath::new(
            format!("/mem/{}/{}.md", project.title.as_ref(), id.as_ref()).into(),
        );
        let record = TaskRecord {
            id: id.clone(),
            title: new.title.to_string(),
            status: TaskStatus::Active,
            created_at: Some(new.created_at),
            completed_at: None,
            commits: None,
            tags: new.tags.map(|tags| render_tags(&tags)),
            effort: new.effort.map(|effort| effort.to_string()),
            priority: new.priority.map(|priority| priority.to_string()),
            blocked_by: new.blocked_by.map_or(
                crate::ports::task_vault::StoredBlockedBy::Absent,
                crate::ports::task_vault::StoredBlockedBy::Valid,
            ),
            section: None,
            body: new.body.clone(),
            source: new.body,
            locator,
            placement: None,
            materialization: Materialization::NoteFile,
            revision,
        };
        tasks.push(record.clone());
        Ok(record)
    }

    fn read_task_markdown(
        &self,
        locator: &pwf_wire::task::TaskNotePath,
    ) -> Result<String, Self::Error> {
        if self
            .lock()
            .failures
            .contains(&InMemoryStoreFailure::ReadTaskMarkdown)
        {
            return Err(InMemoryStoreError::Injected {
                operation: "task-markdown-read",
            });
        }
        self.lock()
            .tasks
            .values()
            .flatten()
            .find(|task| &task.locator == locator)
            .map(|task| task.source.clone())
            .ok_or_else(|| InMemoryStoreError::TaskNoteMarkdownMissing {
                locator: locator.to_string(),
            })
    }

    fn list_index_entries(&self, project: &Project) -> Result<Vec<IndexEntry>, Self::Error> {
        Ok(self.index_entries(project))
    }

    fn upsert_index_entry(&self, project: &Project, entry: IndexEntry) -> Result<(), Self::Error> {
        self.upsert(project, entry);
        Ok(())
    }

    fn commit_task_writes(
        &self,
        project: &Project,
        writes: TaskWriteSet,
    ) -> Result<(), TaskMutationError<Self::Error>> {
        self.commit_task_writes_impl(project, writes)
    }
}

impl InMemoryStore {
    fn commit_task_writes_impl(
        &self,
        project: &Project,
        writes: TaskWriteSet,
    ) -> Result<(), TaskMutationError<InMemoryStoreError>> {
        let mut state = self.lock();
        validate_task_revisions(&state, &project.title, writes.expected())?;

        for write in writes.into_parts().1 {
            apply_task_write(&mut state, &project.title, write)
                .map_err(TaskMutationError::Store)?;
        }
        Ok(())
    }
}

fn apply_task_write(
    state: &mut InMemoryState,
    project: &ProjectName,
    write: TaskWrite,
) -> Result<(), InMemoryStoreError> {
    match write {
        TaskWrite::Patch { id, patch } => {
            let revision = next_task_revision(state);
            let record = state
                .tasks
                .entry(project.clone())
                .or_default()
                .iter_mut()
                .find(|task| task.id == id)
                .ok_or_else(|| InMemoryStoreError::TaskNotFound { id: id.clone() })?;
            apply_task_patch(record, patch);
            record.revision = revision;
        }
        TaskWrite::DeleteNote { id, .. } => {
            let tasks = state.tasks.entry(project.clone()).or_default();
            let before = tasks.len();
            tasks.retain(|task| task.id != id);
            if before == tasks.len() {
                return Err(InMemoryStoreError::TaskNotFound { id });
            }
        }
        TaskWrite::UpsertIndex(entry) => {
            let entries = state.entries.entry(project.clone()).or_default();
            if let Some(existing) = entries.iter_mut().find(|stored| stored.id == entry.id) {
                *existing = entry;
            } else {
                entries.push(entry);
            }
            bump_index_backed_revisions(state, project);
        }
        TaskWrite::DeleteIndex(id) => {
            state
                .entries
                .entry(project.clone())
                .or_default()
                .retain(|entry| entry.id != id);
            bump_index_backed_revisions(state, project);
        }
    }
    Ok(())
}

fn validate_task_revisions(
    state: &InMemoryState,
    project: &ProjectName,
    expected: &[crate::ports::task_vault::ExpectedTaskRevision],
) -> Result<(), TaskMutationError<InMemoryStoreError>> {
    for expectation in expected {
        validate_task_revision(state, project, expectation)?;
    }
    Ok(())
}

fn validate_task_revision(
    state: &InMemoryState,
    project: &ProjectName,
    expected: &crate::ports::task_vault::ExpectedTaskRevision,
) -> Result<(), TaskMutationError<InMemoryStoreError>> {
    let current = state
        .tasks
        .get(project)
        .and_then(|tasks| tasks.iter().find(|task| task.id == expected.id));
    match current {
        Some(record) if record.revision == expected.revision => Ok(()),
        Some(record) => Err(TaskMutationError::StaleTask {
            id: expected.id.clone(),
            expected: expected.revision.clone(),
            current: TaskRevisionState::Present(record.revision.clone()),
        }),
        None => Err(TaskMutationError::StaleTask {
            id: expected.id.clone(),
            expected: expected.revision.clone(),
            current: TaskRevisionState::Missing,
        }),
    }
}

fn apply_task_patch(record: &mut TaskRecord, patch: TaskPatch) {
    patch.status.apply(&mut record.status);
    apply_nullable_patch(&mut record.completed_at, patch.completed_at);
    apply_nullable_patch(&mut record.commits, patch.commits);
    patch.body.apply(&mut record.body);
    patch
        .title
        .map(|title| title.to_string())
        .apply(&mut record.title);
    match patch.blocked_by {
        NullablePatch::Unchanged => {}
        NullablePatch::Clear => record.blocked_by = StoredBlockedBy::Absent,
        NullablePatch::Set(blocked_by) => record.blocked_by = StoredBlockedBy::Valid(blocked_by),
    }
    match patch.effort {
        NullablePatch::Unchanged => {}
        NullablePatch::Clear => record.effort = None,
        NullablePatch::Set(effort) => record.effort = Some(effort.to_string()),
    }
    apply_nullable_patch(
        &mut record.priority,
        patch.priority.map(|priority| priority.to_string()),
    );
    apply_nullable_patch(&mut record.tags, patch.tags.map(|tags| render_tags(&tags)));
}

fn next_task_revision(state: &mut InMemoryState) -> ContentRevision {
    state.next_task_revision = state.next_task_revision.saturating_add(1);
    ContentRevision::try_new(format!("{:064x}", state.next_task_revision)).unwrap()
}

fn bump_index_backed_revisions(state: &mut InMemoryState, project: &ProjectName) {
    let revision = next_task_revision(state);
    for task in state.tasks.entry(project.clone()).or_default() {
        if matches!(task.materialization, Materialization::MissingNote { .. }) {
            task.revision.clone_from(&revision);
        }
    }
}

fn render_tags(tags: &TaskTags) -> RawTaskTags {
    RawTaskTags::new(format!(
        "[{}]",
        tags.iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

fn apply_nullable_patch<T>(target: &mut Option<T>, patch: NullablePatch<T>) {
    match patch {
        NullablePatch::Unchanged => {}
        NullablePatch::Clear => *target = None,
        NullablePatch::Set(value) => *target = Some(value),
    }
}

impl ProjectNotes for InMemoryStore {
    type Error = InMemoryStoreError;

    fn get_note(&self, project: &Project, id: &NoteId) -> Result<Option<ProjectNote>, Self::Error> {
        Ok(self
            .lock()
            .project_notes
            .get(&project.title)
            .and_then(|notes| notes.iter().find(|note| note.id == *id).cloned()))
    }

    fn list_notes(&self, project: &Project) -> Result<Vec<ProjectNote>, Self::Error> {
        if self
            .lock()
            .failures
            .contains(&InMemoryStoreFailure::ListProjectNotes)
        {
            return Err(InMemoryStoreError::Injected {
                operation: "project-note-list",
            });
        }
        Ok(self
            .lock()
            .project_notes
            .get(&project.title)
            .cloned()
            .unwrap_or_default())
    }

    fn insert_note(
        &self,
        project: &Project,
        new: NewProjectNote,
    ) -> Result<ProjectNote, Self::Error> {
        let record = ProjectNote {
            verified: new.verified,
            id: new.id,
            title: new.title,
        };
        let mut state = self.lock();
        state
            .project_note_creations
            .entry(project.title.clone())
            .or_default()
            .push(new.created);
        state
            .project_notes
            .entry(project.title.clone())
            .or_default()
            .push(record.clone());
        Ok(record)
    }

    fn update_note(
        &self,
        project: &Project,
        id: &NoteId,
        patch: ProjectNotePatch,
    ) -> Result<(), Self::Error> {
        let mut state = self.lock();
        state
            .project_note_patches
            .entry(project.title.clone())
            .or_default()
            .push(patch.clone());
        let note = state
            .project_notes
            .entry(project.title.clone())
            .or_default()
            .iter_mut()
            .find(|note| note.id == *id)
            .unwrap();
        drop(patch.verified.apply(&mut note.verified));
        patch.title.apply(&mut note.title);
        Ok(())
    }

    fn delete_note(&self, project: &Project, id: &NoteId) -> Result<(), Self::Error> {
        if self
            .lock()
            .failures
            .contains(&InMemoryStoreFailure::DeleteProjectNote)
        {
            return Err(InMemoryStoreError::Injected {
                operation: "project-note-delete",
            });
        }
        let mut state = self.lock();
        let notes = state
            .project_notes
            .entry(project.title.clone())
            .or_default();
        let count_before = notes.len();
        notes.retain(|note| note.id != *id);
        assert!(count_before > notes.len(), "delete of unknown project note");
        Ok(())
    }
}

impl InMemoryStore {
    fn index_entries(&self, project: &Project) -> Vec<IndexEntry> {
        self.lock()
            .entries
            .get(&project.title)
            .cloned()
            .unwrap_or_default()
    }
}

impl InMemoryStore {
    /// Replaces or appends an index entry.
    fn upsert(&self, project: &Project, entry: IndexEntry) {
        let mut state = self.lock();
        let entries = state.entries.entry(project.title.clone()).or_default();
        match entries.iter_mut().find(|existing| existing.id == entry.id) {
            Some(existing) => *existing = entry,
            None => entries.push(entry),
        }
        bump_index_backed_revisions(&mut state, &project.title);
    }
}
