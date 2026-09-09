use std::collections::{HashMap, HashSet};

use pwf_models::{
    project::ProjectId,
    task::{BlockedBy, TaskId},
};
use pwf_wire::project::{GetProject, ProjectStatusFilter};

use crate::{
    ports::task_vault::{TaskDependencyRecord, TaskVault},
    project::get_project::{self, GetProjectError},
};

pub struct ReadTaskDependencies<'a> {
    pub target: &'a TaskId,
    pub blockers: &'a BlockedBy,
}

#[derive(Debug, thiserror::Error)]
pub enum ReadTaskDependenciesError {
    #[error("cannot read --blocked-by task {id}: {source}")]
    ReadStore {
        id: TaskId,
        #[source]
        source: anyhow::Error,
    },
    #[error(transparent)]
    QueryProject(anyhow::Error),
}

#[cqrsy::query]
pub async fn execute(
    query: ReadTaskDependencies<'_>,
    store: &impl TaskVault,
    pool: &sqlx::SqlitePool,
) -> Result<HashMap<TaskId, TaskDependencyRecord>, ReadTaskDependenciesError> {
    let mut projects = HashMap::<ProjectId, _>::new();
    let mut records = HashMap::new();
    let mut visited = HashSet::new();
    let mut pending = query.blockers.iter().cloned().collect::<Vec<_>>();
    // Each reachable identity is read at most once; the target's outgoing edges are being replaced.
    while let Some(id) = pending.pop() {
        if id == *query.target || !visited.insert(id.clone()) {
            continue;
        }
        let project_id = id.project_id();
        if !projects.contains_key(project_id) {
            let project = match get_project::execute(
                GetProject {
                    id: project_id.clone(),
                    status: ProjectStatusFilter::IncludingPaused,
                },
                pool,
            )
            .await
            {
                Ok(project) => Some(project),
                Err(GetProjectError::ProjectNotFound { .. }) => None,
                Err(error) => {
                    return Err(ReadTaskDependenciesError::QueryProject(anyhow::Error::new(
                        error,
                    )));
                }
            };
            projects.insert(project_id.clone(), project);
        }
        let Some(project) = projects.get(project_id).and_then(Option::as_ref) else {
            continue;
        };
        let record = store
            .get_task_dependencies(project, &id)
            .map_err(|source| ReadTaskDependenciesError::ReadStore {
                id: id.clone(),
                source: anyhow::Error::new(source),
            })?;
        if let Some(record) = record {
            if let Some(blockers) = record.blocked_by.valid() {
                pending.extend(blockers.iter().filter(|id| !visited.contains(*id)).cloned());
            }
            records.insert(id, record);
        }
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        task::read_task_dependencies,
        testing::{
            InMemoryStore, InMemoryStoreFailure, blocked_by, insert_project, stored_blocked_by,
            task_record,
        },
    };

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn reads_only_reachable_dependencies_once_including_paused_projects(
        pool: sqlx::SqlitePool,
    ) {
        insert_project(&pool, "FOO", "foo", "/work/foo", "/tasks/foo", false).await;
        insert_project(&pool, "AUX", "aux", "/work/aux", "/tasks/aux", true).await;
        insert_project(&pool, "BAD", "bad", "/work/bad", "/tasks/bad", false).await;
        sqlx::query("UPDATE projects SET tasks_path = '' WHERE id = 'BAD'")
            .execute(&pool)
            .await
            .unwrap();
        let mut first = task_record("FOO-0001");
        first.blocked_by = stored_blocked_by(&["AUX-0001"]);
        let mut second = task_record("FOO-0002");
        second.blocked_by = stored_blocked_by(&["AUX-0001"]);
        let mut shared = task_record("AUX-0001");
        shared.blocked_by = stored_blocked_by(&["FOO-0001", "FOO-0003"]);
        let store = InMemoryStore::default()
            .with_project(
                "foo",
                vec![
                    first,
                    second,
                    task_record("FOO-0003"),
                    task_record("FOO-0099"),
                ],
            )
            .with_project("aux", vec![shared])
            .with_failure(InMemoryStoreFailure::ReadTaskRecord)
            .with_failure(InMemoryStoreFailure::ListTasks);
        let blockers = blocked_by(&["FOO-0001", "FOO-0002"]);
        let target = "FOO-0003".parse().unwrap();
        let graph = read_task_dependencies::execute(
            ReadTaskDependencies {
                target: &target,
                blockers: &blockers,
            },
            &store,
            &pool,
        )
        .await
        .unwrap();
        let mut reads = store.dependency_reads();
        reads.sort();
        assert_eq!(
            reads.iter().map(AsRef::as_ref).collect::<Vec<_>>(),
            ["AUX-0001", "FOO-0001", "FOO-0002"]
        );
        assert_eq!(graph.len(), 3);
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn missing_projects_and_index_only_tasks_do_not_supply_dependencies(
        pool: sqlx::SqlitePool,
    ) {
        insert_project(&pool, "FOO", "foo", "/work/foo", "/tasks/foo", false).await;
        let (store, _) = crate::testing::staged_missing_task();
        let target = "FOO-0001".parse().unwrap();
        let blockers = blocked_by(&["FOO-0002", "AUX-0001"]);
        let graph = read_task_dependencies::execute(
            ReadTaskDependencies {
                target: &target,
                blockers: &blockers,
            },
            &store,
            &pool,
        )
        .await
        .unwrap();
        assert!(graph.is_empty());
        let error =
            crate::task::blocked_by::validate(&target, &blockers, &blockers, &graph).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Unknown --blocked-by id(s): FOO-0002, AUX-0001."
        );
    }

    #[sqlx::test(migrator = "crate::testing::MIGRATOR")]
    async fn dependency_read_failures_keep_the_task_id_and_source(pool: sqlx::SqlitePool) {
        use std::error::Error as _;
        insert_project(&pool, "FOO", "foo", "/work/foo", "/tasks/foo", false).await;
        let store = InMemoryStore::default().with_failure(InMemoryStoreFailure::ReadTask);
        let target = "FOO-0002".parse().unwrap();
        let blockers = blocked_by(&["FOO-0001"]);
        let error = read_task_dependencies::execute(
            ReadTaskDependencies {
                target: &target,
                blockers: &blockers,
            },
            &store,
            &pool,
        )
        .await
        .unwrap_err();
        assert!(
            matches!(&error, ReadTaskDependenciesError::ReadStore { id, .. } if id.as_ref() == "FOO-0001")
        );
        assert_eq!(
            error.source().unwrap().to_string(),
            "injected in-memory store failure: task-read"
        );
    }
}
