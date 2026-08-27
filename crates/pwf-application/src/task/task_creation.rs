//! Creates a task record and its open index entry.

use pwf_models::{
    project::{Project, ProjectName},
    task::{TaskId, TaskSection, TaskTitle},
};
use pwf_wire::task::TaskNotePath;

use super::normalize_section_label;
use crate::ports::task_record::{
    IndexEntry, IndexEntryState, IndexEntryStore, IndexSectionStore, NewTask, TaskStore,
};

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
    pub fn created_section(&self) -> Option<(&ProjectName, &TaskSection)> {
        match self {
            Self::InsertIndex {
                project,
                created_section: Some(section),
                ..
            } => Some((project, section)),
            _ => None,
        }
    }
}

/// Labels that materialize as dedicated H2 sections.
fn is_dedicated_section(section: &TaskSection) -> bool {
    matches!(section.as_ref(), "Future" | "Human" | "Low-prio")
}

/// Contains a created record and the new H2 section, if one was needed.
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
    store: &(impl TaskStore + IndexEntryStore + IndexSectionStore),
) -> Result<CreatedTask, CreateTaskError> {
    let TaskCreation { project, id, new } = command;
    let target_section = new.section.clone();
    let title = new.title.clone();
    // Read sections before writing so an invalid index leaves no orphaned note.
    let existing = IndexSectionStore::list_index_sections(store, project)
        .map_err(|error| CreateTaskError::ReadSections(anyhow::Error::new(error)))?;
    let created_section = target_section
        .as_ref()
        .filter(|label| is_dedicated_section(label))
        .filter(|label| {
            !existing
                .iter()
                .any(|section| normalize_section_label(section) == **label)
        })
        .cloned();

    let record = TaskStore::insert(store, project, id, new)
        .map_err(|error| CreateTaskError::InsertRecord(anyhow::Error::new(error)))?;
    let id = record.id.clone();
    IndexEntryStore::upsert_index_entry(
        store,
        project,
        IndexEntry {
            id: id.clone(),
            state: IndexEntryState::Open,
            // Preserve the raw label; the adapter owns placement.
            section: target_section,
        },
    )
    .map_err(|error| CreateTaskError::InsertIndex {
        project: project.title.clone(),
        created_section: created_section.clone(),
        source: anyhow::Error::new(error),
    })?;

    Ok(CreatedTask {
        id,
        title,
        note_path: record.locator,
        created_section,
    })
}

#[cfg(test)]
mod tests {
    use pwf_models::{
        project::Project,
        task::{TaskId, TaskTitle},
    };

    use super::{TaskCreation, create};
    use crate::{
        ports::task_record::{IndexEntryState, NewTask},
        testing::{InMemoryStore, app_date, project},
    };

    fn pwf() -> Project {
        project("PWF", "pwf")
    }

    fn new_task(section: Option<&str>) -> NewTask {
        NewTask {
            body: "## Goals\n\n- do the thing".to_string(),
            title: TaskTitle::try_new("ship it").unwrap(),
            created: app_date("2026-07-15"),
            section: section.map(|section| section.parse().unwrap()),
            blocked_by: None,
            effort: None,
            tags: None,
        }
    }

    fn staged_store() -> InMemoryStore {
        InMemoryStore::default().with_project_id("pwf", "PWF")
    }

    fn create_task(store: &InMemoryStore, section: Option<&str>) -> super::CreatedTask {
        let project = pwf();
        let id = TaskId::try_new("PWF-0001").unwrap();
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

        let id = TaskId::try_new("PWF-0001").unwrap();
        assert_eq!(created.id, id.clone());
        assert_eq!(created.created_section, None);
        let tasks = store.tasks("pwf");
        assert_eq!(tasks.len(), 1, "record must be inserted");
        assert_eq!(tasks[0].id, id.clone());
        let entries = store.entries("pwf");
        assert_eq!(entries.len(), 1, "open index entry must be upserted");
        assert_eq!(entries[0].id, id);
        assert_eq!(entries[0].state, IndexEntryState::Open);
        assert_eq!(entries[0].section, None);
    }

    #[test]
    fn create_task_reports_created_section_when_region_absent() {
        let store = staged_store();

        let created = create_task(&store, Some("Human"));

        assert_eq!(
            created.created_section.as_ref().map(AsRef::as_ref),
            Some("Human")
        );
        assert_eq!(
            store.entries("pwf")[0].section.as_ref().map(AsRef::as_ref),
            Some("Human")
        );
    }

    #[test]
    fn create_task_does_not_report_existing_empty_section_region() {
        let store = staged_store().with_sections("pwf", &["Human"]);

        let created = create_task(&store, Some("Human"));

        assert_eq!(created.created_section, None);
    }

    #[test]
    fn create_task_matches_section_aliases_like_the_legacy_read_headers() {
        let store = staged_store().with_sections("pwf", &["Futuro"]);

        let created = create_task(&store, Some("Future"));

        assert_eq!(created.created_section, None);
    }

    #[test]
    fn created_item_carries_record_and_section_fact() {
        let store = staged_store();
        let created = create_task(&store, Some("Low-prio"));
        assert_eq!(created.id, TaskId::try_new("PWF-0001").unwrap());
        assert_eq!(created.title.as_ref(), "ship it");
        assert_eq!(
            created.created_section.as_ref().map(AsRef::as_ref),
            Some("Low-prio")
        );
    }
}
