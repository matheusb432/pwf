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
