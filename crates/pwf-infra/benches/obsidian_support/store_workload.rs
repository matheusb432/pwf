use std::{fmt::Write as _, fs, path::Path};

use pwf_application::ports::task_vault::{
    ExpectedTaskRevision, NewTask, NullablePatch, TaskMutationError, TaskPatch, TaskRecord,
    TaskVault, TaskWrite, TaskWriteSet,
};
use pwf_infra::obsidian::{ObsidianStore, ObsidianStoreError};
use pwf_models::{
    project::{
        HomeDirectory, Project, ProjectId, ProjectName, ProjectSource, ProjectSourceKind,
        ProjectSourceValue, ProjectTasks, ProjectTasksKind, ProjectTasksPath,
    },
    task::{BlockedBy, EffortTier, Tag, TaskId, TaskStatus, TaskTags, TaskTimestamp, TaskTitle},
};
use tempfile::TempDir;

use super::{
    fixture::{DocumentSize, document_body, manifest, require, temporary_directory},
    store_fixture::task_source,
};

pub struct ReadWorkload {
    _directory: TempDir,
    pub store: ObsidianStore,
    pub project: Project,
    pub selected_id: TaskId,
    pub expected_body: String,
}

impl ReadWorkload {
    pub fn single(size: DocumentSize) -> Self {
        Self::with_task_count(size, 1)
    }

    pub fn many() -> Self {
        Self::with_task_count(DocumentSize::Small, manifest().list_task_count)
    }

    pub fn get(&self) -> Result<Option<TaskRecord>, ObsidianStoreError> {
        TaskVault::get_task(&self.store, &self.project, &self.selected_id)
    }

    pub fn list(&self) -> Result<Vec<TaskRecord>, ObsidianStoreError> {
        TaskVault::list_tasks(&self.store, &self.project)
    }

    fn with_task_count(size: DocumentSize, task_count: usize) -> Self {
        let directory = temporary_directory("store-read-");
        let tasks_path = directory.path().join("vault").join("pwf");
        write_vault(&tasks_path, size, task_count);
        let project = project(&tasks_path);
        Self {
            _directory: directory,
            store: ObsidianStore::new(HomeDirectory::new(tasks_path.clone())),
            project,
            selected_id: task_id(1),
            expected_body: format!("\n{}", document_body(size)),
        }
    }
}

pub struct UpdateWorkload {
    read: ReadWorkload,
    patch: Option<TaskPatch>,
}

impl UpdateWorkload {
    pub fn new(size: DocumentSize) -> Self {
        Self {
            read: ReadWorkload::single(size),
            patch: Some(update_patch()),
        }
    }

    pub fn update(&mut self) -> Result<(), TaskMutationError<ObsidianStoreError>> {
        let patch = require_some(self.patch.take(), "taking the one-use benchmark patch");
        let record = self
            .read
            .get()
            .map_err(TaskMutationError::Store)?
            .ok_or_else(|| {
                TaskMutationError::Store(ObsidianStoreError::TaskNotFound {
                    id: self.read.selected_id.clone(),
                })
            })?;
        let writes = TaskWriteSet::try_new(
            vec![ExpectedTaskRevision {
                id: record.id.clone(),
                revision: record.revision,
            }],
            vec![TaskWrite::Patch {
                id: record.id,
                patch,
            }],
        )
        .map_err(|source| {
            TaskMutationError::Store(ObsidianStoreError::TaskMutationFilesystem {
                source: std::io::Error::other(source),
            })
        })?;
        TaskVault::commit_task_writes(&self.read.store, &self.read.project, writes)
    }

    pub fn updated_record(&self) -> Result<TaskRecord, ObsidianStoreError> {
        self.read
            .get()?
            .ok_or_else(|| ObsidianStoreError::TaskNotFound {
                id: self.read.selected_id.clone(),
            })
    }
}

pub struct InsertWorkload {
    _directory: TempDir,
    store: ObsidianStore,
    project: Project,
    id: TaskId,
    new_task: Option<NewTask>,
}

impl InsertWorkload {
    pub fn new(size: DocumentSize) -> Self {
        let directory = temporary_directory("store-insert-");
        let tasks_path = directory.path().join("vault").join("pwf");
        require(
            fs::create_dir_all(&tasks_path),
            "creating insert fixture directory",
        );
        require(
            fs::write(tasks_path.join("pwf.md"), project_index_source(0)),
            "writing insert fixture index",
        );
        let project = project(&tasks_path);
        Self {
            _directory: directory,
            store: ObsidianStore::new(HomeDirectory::new(tasks_path)),
            project,
            id: task_id(1),
            new_task: Some(NewTask {
                body: document_body(size),
                title: require(
                    TaskTitle::try_new("inserted benchmark task"),
                    "constructing benchmark task title",
                ),
                created_at: task_timestamp(),
                blocked_by: Some(blocked_by()),
                effort: Some(EffortTier::Medium),
                priority: None,
                tags: Some(task_tags()),
            }),
        }
    }

    pub fn insert(&mut self) -> Result<TaskRecord, ObsidianStoreError> {
        let new_task = require_some(self.new_task.take(), "taking the one-use benchmark task");
        TaskVault::insert_task(&self.store, &self.project, &self.id, new_task)
    }
}

pub fn validate() {
    for size in [DocumentSize::Small, DocumentSize::Large] {
        let read = ReadWorkload::single(size);
        let record = require_some(
            require(read.get(), "reading benchmark task"),
            "finding benchmark task",
        );
        assert_eq!(record.id, read.selected_id);
        assert_eq!(record.title, "benchmark task 0001");
        assert_eq!(record.body, read.expected_body);

        let mut update = UpdateWorkload::new(size);
        require(update.update(), "updating benchmark task");
        let record = require(update.updated_record(), "reading updated benchmark task");
        assert_eq!(record.status, TaskStatus::Done);
        assert_eq!(record.completed_at, Some(task_timestamp()));
        assert_eq!(record.commits.as_deref(), Some("\"abc123..def456\""));
        assert_eq!(record.effort.as_deref(), Some("high"));
    }

    let read = ReadWorkload::many();
    let records = require(read.list(), "listing benchmark tasks");
    assert_eq!(records.len(), manifest().list_task_count);

    let mut insert = InsertWorkload::new(DocumentSize::Small);
    let record = require(insert.insert(), "inserting benchmark task");
    assert_eq!(record.id.as_ref(), "PWF-0001");
    assert_eq!(record.title, "inserted benchmark task");
}

fn write_vault(tasks_path: &Path, size: DocumentSize, task_count: usize) {
    require(fs::create_dir_all(tasks_path), "creating benchmark vault");
    require(
        fs::write(tasks_path.join("pwf.md"), project_index_source(task_count)),
        "writing benchmark project index",
    );
    for task_number in 1..=task_count {
        require(
            fs::write(
                tasks_path.join(format!("PWF-{task_number:04}.md")),
                task_source(task_number, size),
            ),
            "writing benchmark task note",
        );
    }
}

fn project_index_source(task_count: usize) -> String {
    let mut source = String::from("---\nid: pwf\ntitle: pwf\n---\n\n# Tasks\n\n");
    for task_number in 1..=task_count {
        let _ = writeln!(
            source,
            "- [ ] [[PWF-{task_number:04}|benchmark task {task_number:04}]]"
        );
    }
    source
}

fn project(tasks_path: &Path) -> Project {
    let tasks_path = tasks_path.to_string_lossy().into_owned();
    Project {
        obsidian_vault: None,
        id: require(
            ProjectId::try_new("PWF"),
            "constructing benchmark project ID",
        ),
        title: require(
            ProjectName::try_new("pwf"),
            "constructing benchmark project name",
        ),
        source: Some(ProjectSource::new(
            ProjectSourceKind::Directory,
            require(
                ProjectSourceValue::try_new("/projects/pwf"),
                "constructing benchmark project source",
            ),
        )),
        tasks: ProjectTasks::new(
            ProjectTasksKind::Directory,
            require(
                ProjectTasksPath::try_new(tasks_path),
                "constructing benchmark task path",
            ),
        ),
        created_at: require(
            "2026-08-27T00:00:00Z".parse(),
            "constructing benchmark project timestamp",
        ),
        is_paused: false,
    }
}

fn update_patch() -> TaskPatch {
    TaskPatch {
        status: Some(TaskStatus::Done),
        completed_at: NullablePatch::Set(task_timestamp()),
        commits: NullablePatch::Set("abc123..def456".to_string()),
        body: None,
        title: None,
        blocked_by: NullablePatch::Set(blocked_by()),
        effort: NullablePatch::Set(EffortTier::High),
        priority: NullablePatch::Unchanged,
        tags: NullablePatch::Set(task_tags()),
    }
}

fn blocked_by() -> BlockedBy {
    require(
        BlockedBy::try_new([task_id_for_project("AUX", 1)]),
        "constructing benchmark dependency",
    )
}

fn task_tags() -> TaskTags {
    require(
        TaskTags::try_new(
            ["benchmark", "obsidian"]
                .map(|raw| require(Tag::try_from(raw), "constructing benchmark tag"))
                .to_vec(),
        ),
        "constructing benchmark tags",
    )
}

fn task_id(task_number: usize) -> TaskId {
    task_id_for_project("PWF", task_number)
}

fn task_id_for_project(project: &str, task_number: usize) -> TaskId {
    require(
        TaskId::try_new(format!("{project}-{task_number:04}")),
        "constructing benchmark task ID",
    )
}

fn task_timestamp() -> TaskTimestamp {
    require(
        "2026-08-27T12:34:56Z".parse(),
        "constructing benchmark task timestamp",
    )
}

fn require_some<T>(value: Option<T>, context: &str) -> T {
    if let Some(value) = value {
        value
    } else {
        eprintln!("benchmark setup failed while {context}");
        std::process::exit(1);
    }
}
