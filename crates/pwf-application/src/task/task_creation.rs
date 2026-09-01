//! Creates a task record and its open index entry.

use pwf_models::{
    project::{Project, ProjectName},
    task::{TaskId, TaskSection, TaskTitle},
};
use pwf_wire::task::TaskNotePath;

use super::normalize_section_label;
use crate::ports::task_vault::{IndexEntry, IndexEntryState, NewTask, TaskVault};

/// Reports the persistence phase that failed while creating a task.
#[derive(Debug, thiserror::Error)]
pub enum CreateTaskError {
    #[error(transparent)]
    ReadSections(anyhow::Error),
    #[error(transparent)]
    InsertRecord(anyhow::Error),
    #[error("{source}")]
    InsertIndex {
        project: ProjectName,
        created_section: Option<TaskSection>,
        #[source]
        source: anyhow::Error,
    },
}

impl CreateTaskError {
    #[must_use]
    pub fn created_section(&self) -> Option<(&ProjectName, &TaskSection)> {
        match self {
            Self::InsertIndex {
                project,
                created_section: Some(section),
                ..
            } => Some((project, section)),
            Self::ReadSections(_) | Self::InsertRecord(_) | Self::InsertIndex { .. } => None,
        }
    }
}

/// Describes the record and index effects of one successful creation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::task) struct CreatedTask {
    pub(in crate::task) id: TaskId,
    pub(in crate::task) title: TaskTitle,
    pub(in crate::task) note_path: TaskNotePath,
    pub(in crate::task) created_section: Option<TaskSection>,
}

pub(in crate::task) struct TaskCreation<'a> {
    pub(in crate::task) project: &'a Project,
    pub(in crate::task) id: &'a TaskId,
    pub(in crate::task) new: NewTask,
}

/// Inserts a note record, then upserts its open index entry.
pub(in crate::task) fn create(
    command: TaskCreation<'_>,
    store: &impl TaskVault,
) -> Result<CreatedTask, CreateTaskError> {
    let TaskCreation { project, id, new } = command;
    let target_section = new.section.clone();
    // Read sections before writing so an invalid index leaves no orphaned note.
    let existing = TaskVault::list_index_sections(store, project)
        .map_err(|error| CreateTaskError::ReadSections(anyhow::Error::new(error)))?;
    let created_section = new_section(target_section.as_ref(), &existing);
    let title = new.title.clone();

    let record = TaskVault::insert_task(store, project, id, new)
        .map_err(|error| CreateTaskError::InsertRecord(anyhow::Error::new(error)))?;
    let id = record.id.clone();
    upsert_index(store, project, &id, target_section, created_section.clone())?;
    Ok(CreatedTask {
        id,
        title,
        note_path: record.locator,
        created_section,
    })
}

/// Finishes a previously reserved creation whose note is already observable.
pub(in crate::task) fn ensure_index(
    store: &impl TaskVault,
    project: &Project,
    id: &TaskId,
    section: Option<pwf_models::task::TaskSection>,
) -> Result<(), CreateTaskError> {
    let existing = TaskVault::list_index_sections(store, project)
        .map_err(|error| CreateTaskError::ReadSections(anyhow::Error::new(error)))?;
    let created_section = new_section(section.as_ref(), &existing);
    upsert_index(store, project, id, section, created_section)
}

fn upsert_index(
    store: &impl TaskVault,
    project: &Project,
    id: &TaskId,
    section: Option<pwf_models::task::TaskSection>,
    created_section: Option<TaskSection>,
) -> Result<(), CreateTaskError> {
    TaskVault::upsert_index_entry(
        store,
        project,
        IndexEntry {
            id: id.clone(),
            state: IndexEntryState::Open,
            // Preserve the raw label; the adapter owns placement.
            section,
        },
    )
    .map_err(|error| CreateTaskError::InsertIndex {
        project: project.title.clone(),
        created_section,
        source: anyhow::Error::new(error),
    })
}

pub(in crate::task) fn new_section(
    target: Option<&TaskSection>,
    existing: &[TaskSection],
) -> Option<TaskSection> {
    target
        .filter(|section| matches!(section.as_ref(), "Future" | "Human" | "Low-prio"))
        .filter(|section| {
            !existing
                .iter()
                .any(|candidate| normalize_section_label(candidate) == **section)
        })
        .cloned()
}

#[cfg(test)]
mod tests {
    use pwf_models::{
        project::Project,
        task::{TaskId, TaskTitle},
    };

    use super::{TaskCreation, create};
    use crate::{
        ports::task_vault::{IndexEntryState, NewTask},
        testing::{InMemoryStore, project, task_timestamp},
    };

    fn foo() -> Project {
        project("FOO", "foo")
    }

    fn new_task(section: Option<&str>) -> NewTask {
        NewTask {
            body: "## Goals\n\n- do the thing".to_string(),
            title: TaskTitle::try_new("ship it").unwrap(),
            created_at: task_timestamp("2026-07-15T12:34:56Z"),
            section: section.map(|section| section.parse().unwrap()),
            blocked_by: None,
            effort: None,
            priority: None,
            tags: None,
        }
    }

    fn staged_store() -> InMemoryStore {
        InMemoryStore::default().with_project_id("foo", "FOO")
    }

    fn create_task(store: &InMemoryStore, section: Option<&str>) -> super::CreatedTask {
        let project = foo();
        let id = TaskId::try_new("FOO-0001").unwrap();
        create(
            TaskCreation {
                project: &project,
                id: &id,
                new: new_task(section),
            },
            store,
        )
        .unwrap()
    }

    #[test]
    fn create_task_inserts_record_and_open_index_entry() {
        let store = staged_store();

        let created = create_task(&store, None);

        let id = TaskId::try_new("FOO-0001").unwrap();
        assert_eq!(created.id, id.clone());
        let tasks = store.tasks("foo");
        assert_eq!(tasks.len(), 1, "record must be inserted");
        assert_eq!(tasks[0].id, id.clone());
        let entries = store.entries("foo");
        assert_eq!(entries.len(), 1, "open index entry must be upserted");
        assert_eq!(entries[0].id, id);
        assert_eq!(entries[0].state, IndexEntryState::Open);
        assert_eq!(entries[0].section, None);
    }

    #[test]
    fn create_task_preserves_requested_section_placement() {
        let store = staged_store();

        let created = create_task(&store, Some("Human"));

        assert_eq!(created.id, TaskId::try_new("FOO-0001").unwrap());
        assert_eq!(created.title.as_ref(), "ship it");
        assert_eq!(created.created_section, Some("Human".parse().unwrap()));
        assert_eq!(
            store.entries("foo")[0].section.as_ref().map(AsRef::as_ref),
            Some("Human")
        );
    }
}
