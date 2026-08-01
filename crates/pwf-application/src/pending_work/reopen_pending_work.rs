use pwf_models::pending_work::{ProjectName, WorkItemId, WorkItemStatus};

use super::{
    ProjectRegistry, identifier,
    store_util::{self, LoadItemError},
};
use crate::ports::{
    app_record::AppRecordStore,
    pending_work_record::{IndexEntry, IndexEntryState, ItemPatch, PendingWorkRecord},
};

#[derive(Debug, Clone)]
pub struct ReopenPendingWork {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReopenPendingWorkOk {
    pub id: WorkItemId,
    pub project: ProjectName,
    pub already_active: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ReopenPendingWorkError {
    #[error("Open pending-work item not found: {id}")]
    ItemNotFound { id: String },
    #[error("Unknown task id prefix `{prefix}` for {pending_work_identifier}")]
    UnknownPrefix {
        pending_work_identifier: String,
        prefix: String,
    },
    #[error("{0}")]
    WriteStore(Box<dyn std::error::Error + Send + Sync>),
}

/// Reopens a closed item and restores or re-adds its queue link.
///
/// The update clears `completed:` and `commits:`. An active item returns an idempotent skip.
#[cqrsy::command]
pub fn execute(
    cmd: &ReopenPendingWork,
    store: &(impl AppRecordStore<PendingWorkRecord> + AppRecordStore<IndexEntry>),
    projects: &ProjectRegistry,
) -> Result<ReopenPendingWorkOk, ReopenPendingWorkError> {
    let not_found = || ReopenPendingWorkError::ItemNotFound { id: cmd.id.clone() };
    let pending_work_identifier = identifier::parse(&cmd.id).ok_or_else(not_found)?;
    let project_id = pending_work_identifier
        .as_ref()
        .split_once('-')
        .map_or("", |(project_id, _)| project_id);
    let project = projects
        .get_project_name_by(&pending_work_identifier)
        .ok_or_else(|| ReopenPendingWorkError::UnknownPrefix {
            pending_work_identifier: pending_work_identifier.to_string(),
            prefix: project_id.to_string(),
        })?;
    let record =
        store_util::require_item(store, project, &pending_work_identifier).map_err(|error| {
            match error {
                LoadItemError::ItemNotFound { id } => ReopenPendingWorkError::ItemNotFound { id },
                LoadItemError::Store(source) => ReopenPendingWorkError::WriteStore(source),
            }
        })?;

    if record.status == WorkItemStatus::Active {
        return Ok(ReopenPendingWorkOk {
            id: pending_work_identifier,
            project: project.clone(),
            already_active: true,
        });
    }

    AppRecordStore::<PendingWorkRecord>::update(
        store,
        project,
        &pending_work_identifier,
        ItemPatch {
            status: Some(WorkItemStatus::Active),
            completed: Some(None),
            commits: Some(None),
            ..ItemPatch::default()
        },
    )
    .map_err(|error| ReopenPendingWorkError::WriteStore(Box::new(error)))?;

    let entries = AppRecordStore::<IndexEntry>::list(store, project)
        .map_err(|error| ReopenPendingWorkError::WriteStore(Box::new(error)))?;
    let open_entry = IndexEntry {
        id: pending_work_identifier.clone(),
        state: IndexEntryState::Open,
        section: String::new(),
    };
    match reopen_decision(&entries, &pending_work_identifier) {
        ReopenDecision::RestoreExisting => {
            AppRecordStore::<IndexEntry>::update(
                store,
                project,
                &pending_work_identifier,
                open_entry,
            )
            .map_err(|error| ReopenPendingWorkError::WriteStore(Box::new(error)))?;
        }
        ReopenDecision::ReAddEvicted => {
            AppRecordStore::<IndexEntry>::insert(store, project, open_entry)
                .map_err(|error| ReopenPendingWorkError::WriteStore(Box::new(error)))?;
        }
        ReopenDecision::AlreadyOpen => {}
    }

    Ok(ReopenPendingWorkOk {
        id: pending_work_identifier,
        project: project.clone(),
        already_active: false,
    })
}

#[derive(Clone, Copy)]
enum ReopenDecision {
    RestoreExisting,
    ReAddEvicted,
    AlreadyOpen,
}

fn reopen_decision(entries: &[IndexEntry], id: &WorkItemId) -> ReopenDecision {
    match entries.iter().find(|entry| &entry.id == id) {
        Some(entry) if matches!(entry.state, IndexEntryState::Done(_)) => {
            ReopenDecision::RestoreExisting
        }
        Some(_) => ReopenDecision::AlreadyOpen,
        None => ReopenDecision::ReAddEvicted,
    }
}

#[cfg(test)]
mod tests {
    use pwf_models::pending_work::{ProjectName, Timestamp, WorkItemId, WorkItemStatus};

    use super::{ProjectRegistry, ReopenPendingWork};
    use crate::{
        ports::{
            app_record::AppRecordStore,
            pending_work_record::{
                IndexEntry, IndexEntryState, Materialization, PendingWorkRecord, RecordId,
            },
        },
        testing::InMemoryStore,
    };

    fn registry() -> ProjectRegistry {
        ProjectRegistry::new(vec![(
            ProjectName::try_new("foo-bar").unwrap(),
            Some("/repo".to_string()),
            Some("FOO".to_string()),
        )])
    }

    fn record(id: &str, status: WorkItemStatus) -> PendingWorkRecord {
        PendingWorkRecord {
            id: RecordId::Item(WorkItemId::try_new(id).unwrap()),
            title: "tray gui".to_string(),
            status,
            created: Some(Timestamp::new("2026-01-01")),
            completed: (status != WorkItemStatus::Active).then(|| Timestamp::new("2026-01-02")),
            commits: Some("a..b".to_string()),
            tags: None,
            effort: None,
            prereq: None,
            section: None,
            body: "\nbody\n".to_string(),
            source: "body".to_string(),
            locator: format!("/mem/foo-bar/{id}.md"),
            placement: None,
            materialization: Materialization::NoteFile,
        }
    }

    fn foo() -> ProjectName {
        ProjectName::try_new("foo-bar").unwrap()
    }

    fn staged(status: WorkItemStatus, entries: Vec<IndexEntry>) -> InMemoryStore {
        let store = InMemoryStore::default()
            .with_prefix("foo-bar", "FOO")
            .with_project("foo-bar", vec![record("FOO-0001", status)]);
        for entry in entries {
            <InMemoryStore as AppRecordStore<IndexEntry>>::insert(&store, &foo(), entry).unwrap();
        }
        store
    }

    fn entry(state: IndexEntryState) -> IndexEntry {
        IndexEntry {
            id: WorkItemId::try_new("FOO-0001").unwrap(),
            state,
            section: String::new(),
        }
    }

    fn command() -> ReopenPendingWork {
        ReopenPendingWork {
            id: "FOO-0001".to_string(),
        }
    }

    #[test]
    fn reopen_restores_done_entry() {
        let store = staged(
            WorkItemStatus::Done,
            vec![entry(IndexEntryState::Done(Timestamp::new("2026-01-02")))],
        );

        let out = super::execute(&command(), &store, &registry()).unwrap();

        assert!(!out.already_active);
        assert_eq!(store.items("foo-bar")[0].status, WorkItemStatus::Active);
        assert_eq!(store.items("foo-bar")[0].completed, None);
        assert_eq!(store.items("foo-bar")[0].commits, None);
        assert_eq!(store.entries("foo-bar")[0].state, IndexEntryState::Open);
    }

    #[test]
    fn reopen_re_adds_evicted_entry() {
        let store = staged(WorkItemStatus::Done, Vec::new());

        let out = super::execute(&command(), &store, &registry()).unwrap();

        assert!(!out.already_active);
        let entries = store.entries("foo-bar");
        assert_eq!(entries.len(), 1, "evicted link must be re-added");
        assert_eq!(entries[0].id, WorkItemId::try_new("FOO-0001").unwrap());
        assert_eq!(entries[0].state, IndexEntryState::Open);
    }

    #[test]
    fn reopen_already_active_is_idempotent_skip() {
        let store = staged(WorkItemStatus::Active, vec![entry(IndexEntryState::Open)]);

        let out = super::execute(&command(), &store, &registry()).unwrap();

        assert!(out.already_active);
        assert_eq!(store.items("foo-bar")[0].commits.as_deref(), Some("a..b"));
    }

    #[test]
    fn reopen_reports_an_unknown_configured_prefix() {
        let store = staged(WorkItemStatus::Done, Vec::new());
        let command = ReopenPendingWork {
            id: "XYZ-0001".to_string(),
        };

        let error = super::execute(&command, &store, &registry()).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Unknown task id prefix `XYZ` for XYZ-0001"
        );
    }
}
