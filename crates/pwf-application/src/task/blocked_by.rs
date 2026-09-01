use std::collections::HashMap;

use pwf_models::{
    project::{Project, ProjectId},
    task::{BlockedBy, TaskId},
};
use pwf_wire::task::{BlockedByResolution, BlockedByStatus};

use crate::ports::task_vault::{Materialization, StoredBlockedBy, TaskRecord, TaskVault};

#[derive(Debug, thiserror::Error)]
pub(in crate::task) enum BlockedByValidationError {
    #[error("Unknown --blocked-by id(s): {}.", format_task_ids(ids))]
    UnknownIds { ids: Vec<TaskId> },
    #[error("cannot read --blocked-by task {id}: {source}")]
    ReadStore {
        id: TaskId,
        #[source]
        source: anyhow::Error,
    },
    #[error("task {target} cannot be blocked by itself ({blocker})")]
    SelfDependency { target: TaskId, blocker: TaskId },
    #[error("blocked_by cycle: {}", format_task_ids_path(path))]
    Cycle { path: Vec<TaskId> },
    #[error("task {task} at {path} has malformed blocked_by metadata {raw:?}: {reason}")]
    MalformedMetadata {
        task: TaskId,
        path: Box<pwf_wire::task::TaskNotePath>,
        raw: Box<str>,
        reason: Box<str>,
    },
}

pub(in crate::task) fn format_task_ids(ids: &[TaskId]) -> String {
    ids.iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

pub(in crate::task) fn format_task_ids_path(ids: &[TaskId]) -> String {
    ids.iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" -> ")
}

pub(in crate::task) fn validate_and_merge(
    target: &TaskId,
    existing: Option<&BlockedBy>,
    blocked_by: &BlockedBy,
    store: &impl TaskVault,
    projects: &[Project],
) -> Result<BlockedBy, BlockedByValidationError> {
    let merged = existing.map_or_else(|| blocked_by.clone(), |existing| existing.merge(blocked_by));
    validate(target, &merged, blocked_by, store, projects)?;
    Ok(merged)
}

pub(in crate::task) fn validate(
    target: &TaskId,
    final_blockers: &BlockedBy,
    supplied: &BlockedBy,
    store: &impl TaskVault,
    projects: &[Project],
) -> Result<(), BlockedByValidationError> {
    if final_blockers.iter().any(|blocker| blocker == target) {
        return Err(BlockedByValidationError::SelfDependency {
            target: target.clone(),
            blocker: target.clone(),
        });
    }

    let mut unknown = Vec::new();
    for identifier in supplied.iter() {
        let Some(project) = find_project(None, projects, identifier.project_id()) else {
            unknown.push(identifier.clone());
            continue;
        };
        let record = store.get_task(project, identifier).map_err(|source| {
            BlockedByValidationError::ReadStore {
                id: identifier.clone(),
                source: anyhow::Error::new(source),
            }
        })?;
        if record.is_none_or(|record| !matches!(record.materialization, Materialization::NoteFile))
        {
            unknown.push(identifier.clone());
        }
    }
    if !unknown.is_empty() {
        return Err(BlockedByValidationError::UnknownIds { ids: unknown });
    }

    let mut states = HashMap::new();
    let mut path = vec![target.clone()];
    for blocker in final_blockers.iter() {
        visit(blocker, target, store, projects, &mut states, &mut path)?;
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum VisitState {
    Visiting,
    Complete,
}

fn visit(
    id: &TaskId,
    target: &TaskId,
    store: &impl TaskVault,
    projects: &[Project],
    states: &mut HashMap<TaskId, VisitState>,
    path: &mut Vec<TaskId>,
) -> Result<(), BlockedByValidationError> {
    if id == target {
        path.push(target.clone());
        return Err(BlockedByValidationError::Cycle { path: path.clone() });
    }
    match states.get(id) {
        Some(VisitState::Complete) => return Ok(()),
        Some(VisitState::Visiting) => {
            let start = path
                .iter()
                .position(|candidate| candidate == id)
                .unwrap_or(0);
            let mut cycle = path[start..].to_vec();
            cycle.push(id.clone());
            return Err(BlockedByValidationError::Cycle { path: cycle });
        }
        None => {}
    }

    states.insert(id.clone(), VisitState::Visiting);
    path.push(id.clone());
    let record =
        match find_project(None, projects, id.project_id()) {
            Some(project) => store.get_task(project, id).map_err(|source| {
                BlockedByValidationError::ReadStore {
                    id: id.clone(),
                    source: anyhow::Error::new(source),
                }
            })?,
            None => None,
        };
    if let Some(record) = record {
        visit_record(record, target, store, projects, states, path)?;
    }
    path.pop();
    states.insert(id.clone(), VisitState::Complete);
    Ok(())
}

fn visit_record(
    record: TaskRecord,
    target: &TaskId,
    store: &impl TaskVault,
    projects: &[Project],
    states: &mut HashMap<TaskId, VisitState>,
    path: &mut Vec<TaskId>,
) -> Result<(), BlockedByValidationError> {
    if !matches!(record.materialization, Materialization::NoteFile) {
        return Ok(());
    }
    match record.blocked_by {
        StoredBlockedBy::Absent => Ok(()),
        StoredBlockedBy::Valid(blocked_by) => blocked_by
            .iter()
            .try_for_each(|blocker| visit(blocker, target, store, projects, states, path)),
        StoredBlockedBy::Malformed { raw, reason } => {
            Err(BlockedByValidationError::MalformedMetadata {
                task: record.id,
                path: Box::new(record.locator),
                raw: raw.into_boxed_str(),
                reason: reason.into_boxed_str(),
            })
        }
    }
}

pub(in crate::task) fn statuses(
    blocked_by: &BlockedBy,
    store: &impl TaskVault,
    primary_project: Option<&Project>,
    projects: &[Project],
) -> Vec<BlockedByStatus> {
    blocked_by
        .iter()
        .map(|id| status(store, primary_project, projects, id))
        .collect()
}

fn status(
    store: &impl TaskVault,
    primary_project: Option<&Project>,
    projects: &[Project],
    id: &TaskId,
) -> BlockedByStatus {
    let missing = || BlockedByStatus {
        id: id.clone(),
        title: None,
        resolution: BlockedByResolution::Missing,
    };
    let Some(project) = find_project(primary_project, projects, id.project_id()) else {
        return missing();
    };
    let task = match store.get_task(project, id) {
        Ok(task) => task,
        Err(source) => {
            return BlockedByStatus {
                id: id.clone(),
                title: None,
                resolution: BlockedByResolution::Unavailable {
                    reason: source.to_string(),
                },
            };
        }
    };
    let Some(task) = task else {
        return missing();
    };
    if matches!(task.materialization, Materialization::MissingNote { .. }) {
        return missing();
    }
    BlockedByStatus {
        id: id.clone(),
        title: (!task.title.trim().is_empty()).then_some(task.title),
        resolution: BlockedByResolution::Found(task.status),
    }
}

fn find_project<'project>(
    primary_project: Option<&'project Project>,
    projects: &'project [Project],
    id: &ProjectId,
) -> Option<&'project Project> {
    primary_project
        .filter(|project| &project.id == id)
        .or_else(|| projects.iter().find(|project| &project.id == id))
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use pwf_wire::task::BlockedByResolution;

    use super::{BlockedByValidationError, statuses, validate_and_merge};
    use crate::{
        ports::task_vault::TaskRecord,
        testing::{
            InMemoryStore, InMemoryStoreFailure, blocked_by, project, staged_missing_task,
            staged_task, stored_blocked_by, task_record,
        },
    };

    #[test]
    fn validator_parses_checks_existence_and_merges_first_seen_ids() {
        let (store, _registry) = staged_task();
        let project = project("FOO", "foo");
        let target = "FOO-0002".parse().unwrap();
        let existing = blocked_by(&["FOO-0001", "FOO-0001"]);
        let merged = validate_and_merge(
            &target,
            Some(&existing),
            &blocked_by(&["FOO-0001", "FOO-0001"]),
            &store,
            &[project],
        )
        .unwrap();

        assert_eq!(
            merged.iter().map(AsRef::as_ref).collect::<Vec<_>>(),
            ["FOO-0001"]
        );
    }

    #[test]
    fn validator_preserves_blocked_by_diagnostics() {
        let (store, _registry) = staged_task();
        let project = project("FOO", "foo");
        let target = "FOO-0002".parse().unwrap();

        let unknown = validate_and_merge(
            &target,
            None,
            &blocked_by(&["FOO-9999"]),
            &store,
            &[project],
        )
        .unwrap_err();
        assert!(matches!(
            unknown,
            BlockedByValidationError::UnknownIds { ref ids }
                if ids.iter().map(AsRef::as_ref).collect::<Vec<_>>() == ["FOO-9999"]
        ));
        assert_eq!(unknown.to_string(), "Unknown --blocked-by id(s): FOO-9999.");
    }

    #[test]
    fn validator_rejects_a_self_dependency_before_lookup() {
        let target = "FOO-0002".parse().unwrap();
        let blockers = blocked_by(&["FOO-0002"]);

        let error = super::validate(
            &target,
            &blockers,
            &blockers,
            &InMemoryStore::default(),
            &[],
        )
        .unwrap_err();

        assert!(matches!(
            error,
            BlockedByValidationError::SelfDependency { ref target, ref blocker }
                if target.as_ref() == "FOO-0002" && blocker.as_ref() == "FOO-0002"
        ));
    }

    #[test]
    fn validator_reports_an_existing_reachable_cycle() {
        let first = TaskRecord {
            blocked_by: stored_blocked_by(&["FOO-0002"]),
            ..task_record("FOO-0001")
        };
        let second = TaskRecord {
            blocked_by: stored_blocked_by(&["FOO-0001"]),
            ..task_record("FOO-0002")
        };
        let store = InMemoryStore::default().with_project("foo", vec![first, second]);
        let projects = [project("FOO", "foo")];
        let target = "FOO-0003".parse().unwrap();
        let blockers = blocked_by(&["FOO-0001"]);

        let error = super::validate(&target, &blockers, &blockers, &store, &projects).unwrap_err();

        assert!(matches!(
            error,
            BlockedByValidationError::Cycle { ref path }
                if path.iter().map(AsRef::as_ref).collect::<Vec<_>>()
                    == ["FOO-0001", "FOO-0002", "FOO-0001"]
        ));
    }

    #[test]
    fn validator_preserves_store_read_failures() {
        let project = project("FOO", "foo");
        let target = "FOO-0002".parse().unwrap();
        let store = InMemoryStore::default().with_failure(InMemoryStoreFailure::ReadTask);

        let error = validate_and_merge(
            &target,
            None,
            &blocked_by(&["FOO-0001"]),
            &store,
            &[project],
        )
        .unwrap_err();

        assert!(matches!(
            error,
            BlockedByValidationError::ReadStore { ref id, .. } if id.as_ref() == "FOO-0001"
        ));
        assert_eq!(
            error.source().unwrap().to_string(),
            "injected in-memory store failure: task-read"
        );
    }

    #[test]
    fn validator_rejects_missing_note_materializations_as_unknown() {
        let (store, _registry) = staged_missing_task();
        let project = project("FOO", "foo");
        let target = "FOO-0001".parse().unwrap();

        let error = validate_and_merge(
            &target,
            None,
            &blocked_by(&["FOO-0002"]),
            &store,
            &[project],
        )
        .unwrap_err();

        assert!(matches!(
            error,
            BlockedByValidationError::UnknownIds { ref ids }
                if ids.iter().map(AsRef::as_ref).collect::<Vec<_>>() == ["FOO-0002"]
        ));
    }

    #[test]
    fn statuses_expose_store_read_failures_as_unavailable() {
        let project = project("FOO", "foo");
        let store = InMemoryStore::default().with_failure(InMemoryStoreFailure::ReadTask);

        let statuses = statuses(&blocked_by(&["FOO-0001"]), &store, Some(&project), &[]);

        assert!(matches!(
            statuses.as_slice(),
            [super::BlockedByStatus {
                id,
                resolution: BlockedByResolution::Unavailable { reason },
                ..
            }] if id.as_ref() == "FOO-0001"
                && reason == "injected in-memory store failure: task-read"
        ));
    }
}
