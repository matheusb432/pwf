//! Creates a task record and its open index entry.

use pwf_models::{project::Project, task::ProjectName};

use super::normalize_section_label;
use crate::ports::task_record::{
    IndexEntry, IndexEntryState, IndexEntryStore, IndexSectionStore, NewTask, TaskRecord, TaskStore,
};

/// Reports the persistence phase that failed while creating a task.
#[derive(Debug, thiserror::Error)]
pub enum CreateTaskError {
    #[error("{0}")]
    ReadSections(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("{0}")]
    InsertRecord(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("{source}")]
    InsertIndex {
        project: ProjectName,
        created_section: Option<String>,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl CreateTaskError {
    pub fn created_section(&self) -> Option<(&ProjectName, &str)> {
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
const SECTION_LABELS: [&str; 3] = ["Future", "Human", "Low-prio"];

/// Contains a created record and the new H2 section, if one was needed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::task) struct CreatedTask {
    pub(in crate::task) record: TaskRecord,
    pub(in crate::task) created_section: Option<String>,
}

pub(in crate::task) struct CreateTask<'a> {
    pub(in crate::task) project: &'a Project,
    pub(in crate::task) new: NewTask,
}

/// Inserts a note record, then upserts its open index entry.
///
/// # Panics
///
/// Panics if the store's `insert` violates its contract by returning a record
/// without a [`TaskId`].
pub(in crate::task) fn execute(
    command: CreateTask<'_>,
    store: &(impl TaskStore + IndexEntryStore + IndexSectionStore),
) -> Result<CreatedTask, CreateTaskError> {
    let CreateTask { project, new } = command;
    let target_section = new.section.clone();
    // Read sections before writing so an invalid index leaves no orphaned note.
    let existing = IndexSectionStore::list_index_sections(store, project)
        .map_err(|error| CreateTaskError::ReadSections(Box::new(error)))?;
    let created_section = target_section
        .as_deref()
        .filter(|label| SECTION_LABELS.contains(label))
        .filter(|label| {
            !existing
                .iter()
                .any(|section| normalize_section_label(&section.label) == *label)
        })
        .map(str::to_string);

    let record = TaskStore::insert(store, project, new)
        .map_err(|error| CreateTaskError::InsertRecord(Box::new(error)))?;
    let id = record.id.clone();
    IndexEntryStore::upsert_index_entry(
        store,
        project,
        IndexEntry {
            id,
            state: IndexEntryState::Open,
            // Preserve the raw label; the adapter owns placement.
            section: target_section.unwrap_or_default(),
        },
    )
    .map_err(|error| CreateTaskError::InsertIndex {
        project: project.title.clone(),
        created_section: created_section.clone(),
        source: Box::new(error),
    })?;

    Ok(CreatedTask {
        record,
        created_section,
    })
}

#[cfg(test)]
mod tests {
    use pwf_models::{
        project::Project,
        task::{TaskId, TaskTitle, Timestamp},
    };

    use super::{CreateTask, execute};
    use crate::{
        ports::task_record::{IndexEntryState, NewTask},
        testing::{InMemoryStore, project},
    };

    fn pwf() -> Project {
        project("PWF", "pwf")
    }

    fn new_task(section: Option<&str>) -> NewTask {
        NewTask {
            prompt: "do the thing".to_string(),
            title: TaskTitle::try_new("ship it").unwrap(),
            created: Timestamp::new("2026-07-15"),
            section: section.map(str::to_string),
            prereq: None,
            effort: None,
            tags: None,
        }
    }

    fn staged_store() -> InMemoryStore {
        InMemoryStore::default().with_project_id("pwf", "PWF")
    }

    #[test]
    fn create_task_inserts_record_and_open_index_entry() {
        let store = staged_store();

        let project = pwf();
        let created = execute(
            CreateTask {
                project: &project,
                new: new_task(None),
            },
            &store,
        )
        .unwrap();

        let id = TaskId::try_new("PWF-0001").unwrap();
        assert_eq!(created.record.id, id.clone());
        assert_eq!(created.created_section, None);
        let tasks = store.tasks("pwf");
        assert_eq!(tasks.len(), 1, "record must be inserted");
        assert_eq!(tasks[0].id, id.clone());
        let entries = store.entries("pwf");
        assert_eq!(entries.len(), 1, "open index entry must be upserted");
        assert_eq!(entries[0].id, id);
        assert_eq!(entries[0].state, IndexEntryState::Open);
        assert_eq!(entries[0].section, "");
    }

    #[test]
    fn create_task_reports_created_section_when_region_absent() {
        let store = staged_store();

        let project = pwf();
        let created = execute(
            CreateTask {
                project: &project,
                new: new_task(Some("Human")),
            },
            &store,
        )
        .unwrap();

        assert_eq!(created.created_section.as_deref(), Some("Human"));
        assert_eq!(store.entries("pwf")[0].section, "Human");
    }

    #[test]
    fn create_task_does_not_report_existing_empty_section_region() {
        let store = staged_store().with_sections("pwf", &["Human"]);

        let project = pwf();
        let created = execute(
            CreateTask {
                project: &project,
                new: new_task(Some("Human")),
            },
            &store,
        )
        .unwrap();

        assert_eq!(created.created_section, None);
    }

    #[test]
    fn create_task_matches_section_aliases_like_the_legacy_read_headers() {
        let store = staged_store().with_sections("pwf", &["Futuro"]);

        let project = pwf();
        let created = execute(
            CreateTask {
                project: &project,
                new: new_task(Some("Future")),
            },
            &store,
        )
        .unwrap();

        assert_eq!(created.created_section, None);
    }

    #[test]
    fn created_item_carries_record_and_section_fact() {
        let store = staged_store();
        let project = pwf();
        let created = execute(
            CreateTask {
                project: &project,
                new: new_task(Some("Low-prio")),
            },
            &store,
        )
        .unwrap();
        assert_eq!(created.record.id, TaskId::try_new("PWF-0001").unwrap());
        assert_eq!(created.record.title, "ship it");
        assert_eq!(created.created_section.as_deref(), Some("Low-prio"));
    }
}
