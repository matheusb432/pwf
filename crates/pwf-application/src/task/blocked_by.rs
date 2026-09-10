use std::collections::HashMap;

use pwf_models::{
    project::{Project, ProjectId},
    task::{BlockedBy, TaskId},
};
use pwf_wire::task::{BlockedByResolution, BlockedByStatus, StoredBlockedBy};

use crate::ports::task_vault::{TaskDependencyRecord, TaskVault};

#[derive(Debug, thiserror::Error)]
pub(in crate::task) enum BlockedByValidationError {
    #[error("Unknown --blocked-by id(s): {}.", format_task_ids(ids))]
    UnknownIds { ids: Vec<TaskId> },
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

pub(in crate::task) fn validate(
    target: &TaskId,
    final_blockers: &BlockedBy,
    supplied: &BlockedBy,
    dependencies: &HashMap<TaskId, TaskDependencyRecord>,
) -> Result<(), BlockedByValidationError> {
    if final_blockers.iter().any(|blocker| blocker == target) {
        return Err(BlockedByValidationError::SelfDependency {
            target: target.clone(),
            blocker: target.clone(),
        });
    }
    let unknown = supplied
        .iter()
        .filter(|id| !dependencies.contains_key(*id))
        .cloned()
        .collect::<Vec<_>>();
    if !unknown.is_empty() {
        return Err(BlockedByValidationError::UnknownIds { ids: unknown });
    }
    let mut complete = std::collections::HashSet::new();
    let mut path = vec![target];
    let mut pending = final_blockers
        .iter()
        .map(|id| (id, false))
        .collect::<Vec<_>>();
    pending.reverse();
    while let Some((id, leaving)) = pending.pop() {
        if leaving {
            path.pop();
            complete.insert(id);
            continue;
        }
        if let Some(start) = path.iter().position(|candidate| *candidate == id) {
            let mut cycle = path[start..]
                .iter()
                .map(|id| (*id).clone())
                .collect::<Vec<_>>();
            cycle.push(id.clone());
            return Err(BlockedByValidationError::Cycle { path: cycle });
        }
        if complete.contains(id) {
            continue;
        }
        let Some(record) = dependencies.get(id) else {
            continue;
        };
        match &record.blocked_by {
            StoredBlockedBy::Absent => {
                complete.insert(id);
            }
            StoredBlockedBy::Valid(blockers) => {
                path.push(id);
                pending.push((id, true));
                let start = pending.len();
                pending.extend(blockers.iter().map(|id| (id, false)));
                pending[start..].reverse();
            }
            StoredBlockedBy::Malformed { raw, reason } => {
                return Err(BlockedByValidationError::MalformedMetadata {
                    task: id.clone(),
                    path: Box::new(record.locator.clone()),
                    raw: raw.clone().into_boxed_str(),
                    reason: reason.clone().into_boxed_str(),
                });
            }
        }
    }
    Ok(())
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
    let task = match store.get_task_record(project, id) {
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
    use super::*;
    use crate::testing::{blocked_by, stored_blocked_by};

    fn dependencies(edges: &[(&str, &[&str])]) -> HashMap<TaskId, TaskDependencyRecord> {
        edges
            .iter()
            .map(|(id, blockers)| {
                (
                    id.parse().unwrap(),
                    TaskDependencyRecord {
                        blocked_by: if blockers.is_empty() {
                            StoredBlockedBy::Absent
                        } else {
                            stored_blocked_by(blockers)
                        },
                        locator: pwf_wire::task::TaskNotePath::new(
                            format!("/tasks/{id}.md").into(),
                        ),
                    },
                )
            })
            .collect()
    }

    #[test]
    fn validator_preserves_unknown_and_self_dependency_diagnostics() {
        let target = "FOO-0002".parse().unwrap();
        let blockers = blocked_by(&["FOO-9999"]);
        let error = validate(&target, &blockers, &blockers, &HashMap::new()).unwrap_err();
        assert_eq!(error.to_string(), "Unknown --blocked-by id(s): FOO-9999.");
        let blockers = blocked_by(&["FOO-0002"]);
        assert!(matches!(
            validate(&target, &blockers, &blockers, &HashMap::new()),
            Err(BlockedByValidationError::SelfDependency { .. })
        ));
    }

    #[test]
    fn validator_reports_existing_and_proposed_cycles() {
        let target = "FOO-0003".parse().unwrap();
        let blockers = blocked_by(&["FOO-0001"]);
        for (last, expected) in [
            (
                "FOO-0001",
                "blocked_by cycle: FOO-0001 -> FOO-0002 -> FOO-0001",
            ),
            (
                "FOO-0003",
                "blocked_by cycle: FOO-0003 -> FOO-0001 -> FOO-0002 -> FOO-0003",
            ),
        ] {
            let graph = dependencies(&[("FOO-0001", &["FOO-0002"]), ("FOO-0002", &[last])]);
            assert_eq!(
                validate(&target, &blockers, &blockers, &graph)
                    .unwrap_err()
                    .to_string(),
                expected
            );
        }
    }

    #[test]
    fn validator_accepts_shared_descendants_and_missing_stored_descendants() {
        let target = "FOO-0005".parse().unwrap();
        let blockers = blocked_by(&["FOO-0001", "FOO-0002"]);
        let graph = dependencies(&[("FOO-0001", &["FOO-0003"]), ("FOO-0002", &["FOO-0003"])]);
        validate(&target, &blockers, &blockers, &graph).unwrap();
    }

    #[test]
    fn validator_rejects_reachable_malformed_metadata() {
        let target = "FOO-0002".parse().unwrap();
        let blockers = blocked_by(&["FOO-0001"]);
        let mut graph = dependencies(&[("FOO-0001", &[])]);
        graph
            .get_mut(&"FOO-0001".parse().unwrap())
            .unwrap()
            .blocked_by = StoredBlockedBy::Malformed {
            raw: "broken".to_string(),
            reason: "invalid metadata".to_string(),
        };
        assert!(matches!(
            validate(&target, &blockers, &blockers, &graph),
            Err(BlockedByValidationError::MalformedMetadata { .. })
        ));
    }
}
