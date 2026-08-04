use pwf_models::{
    project::{Project, ProjectId},
    task::TaskId,
};

use crate::{
    ports::task_record::{TaskRecord, TaskStore},
    project::{
        get_active_project::{self, GetActiveProject},
        get_project::GetProjectError,
    },
    task::{dto::TaskView, logic::finding::find_active_task_in_projects},
};

#[derive(Debug, Clone)]
pub struct FindActiveTask {
    pub id: TaskId,
}

pub struct FindActiveTaskOk {
    pub project: Project,
    pub record: TaskRecord,
    pub task: TaskView,
}

#[derive(Debug, thiserror::Error)]
pub enum FindActiveTaskError {
    #[error("Active task not found: {id}")]
    TaskNotFound { id: TaskId },
    #[error("Task id is ambiguous: {id}")]
    AmbiguousId { id: TaskId },
    #[error("Unknown project ID `{project_id}` for task {task_id}")]
    UnknownProjectId {
        task_id: TaskId,
        project_id: ProjectId,
    },
    #[error("{0}")]
    ReadStore(Box<dyn std::error::Error + Send + Sync>),
    #[error("{0}")]
    QueryProject(Box<dyn std::error::Error + Send + Sync>),
}

#[cqrsy::query]
pub async fn execute(
    query: &FindActiveTask,
    store: &impl TaskStore,
    pool: &sqlx::SqlitePool,
) -> Result<FindActiveTaskOk, FindActiveTaskError> {
    let project_id = query.id.project_id();
    let project = get_active_project::execute(
        GetActiveProject {
            id: project_id.clone(),
        },
        pool,
    )
    .await
    .map_err(|error| match error {
        GetProjectError::ProjectNotFound { .. } => FindActiveTaskError::UnknownProjectId {
            task_id: query.id.clone(),
            project_id,
        },
        error @ GetProjectError::Unexpected { .. } => {
            FindActiveTaskError::QueryProject(Box::new(error))
        }
    })?;
    let (record, task) =
        find_active_task_in_projects(store, std::slice::from_ref(&project), &query.id)?;
    Ok(FindActiveTaskOk {
        project,
        record,
        task,
    })
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use pwf_models::{
        project::Project,
        task::{TaskId, TaskStatus, Timestamp},
    };

    use super::{FindActiveTaskError, TaskView};
    use crate::{
        ports::task_record::{IndexPlacement, Materialization, TaskRecord},
        task::logic::finding::find_active_task_in_projects,
        testing::{InMemoryStore, project},
    };

    fn record(id: &str) -> TaskRecord {
        TaskRecord {
            id: TaskId::try_new(id).unwrap(),
            title: format!("title {id}"),
            status: TaskStatus::Active,
            created: Some(Timestamp::new("2026-07-07")),
            completed: None,
            commits: None,
            tags: None,
            effort: None,
            prereq: None,
            section: None,
            body: "do the thing".to_string(),
            source: String::new(),
            locator: format!("/notes/pwf/{id}.md"),
            placement: Some(IndexPlacement {
                index_path: "/notes/pwf/pwf.md".to_string(),
                line: 7,
            }),
            materialization: Materialization::NoteFile,
        }
    }

    fn projects(values: &[(&str, &str)]) -> Vec<Project> {
        values
            .iter()
            .map(|(name, project_id)| project(project_id.parse().unwrap(), name))
            .collect()
    }

    fn find(
        store: &InMemoryStore,
        projects: &[Project],
        id: &str,
    ) -> Result<TaskView, FindActiveTaskError> {
        find_active_task_in_projects(store, projects, &TaskId::try_new(id).unwrap())
            .map(|(_, task)| task)
    }

    #[test]
    fn finds_open_item_enriched_with_launchability() {
        let store = InMemoryStore::default().with_project("pwf", vec![record("PWF-0001")]);
        let projects = projects(&[("pwf", "PWF")]);

        let task = find(&store, &projects, "PWF-0001").unwrap();

        assert_eq!(task.id.as_ref(), "PWF-0001");
        assert_eq!(task.project, "pwf");
        assert_eq!(task.project_path.as_ref(), "/work/pwf");
        assert_eq!(task.prompt, "do the thing");
        assert!(task.launchable);
    }

    #[test]
    fn missing_task_reports_the_typed_id() {
        let store = InMemoryStore::default().with_project("pwf", vec![record("PWF-0001")]);
        let projects = projects(&[("pwf", "PWF")]);

        let error = find(&store, &projects, "PWF-9999").unwrap_err();

        assert!(matches!(
            error,
            FindActiveTaskError::TaskNotFound { ref id } if id.as_ref() == "PWF-9999"
        ));
        assert_eq!(error.to_string(), "Active task not found: PWF-9999");
    }

    #[test]
    fn duplicate_link_in_one_project_is_ambiguous() {
        let store = InMemoryStore::default()
            .with_project("pwf", vec![record("PWF-0001"), record("PWF-0001")]);
        let projects = projects(&[("pwf", "PWF")]);

        let error = find(&store, &projects, "PWF-0001").unwrap_err();

        assert!(matches!(error, FindActiveTaskError::AmbiguousId { .. }));
    }

    #[test]
    fn find_accepts_unlinked_active_and_rejects_closed_records() {
        let done = TaskRecord {
            status: TaskStatus::Done,
            placement: None,
            ..record("PWF-0001")
        };
        let unlinked_active = TaskRecord {
            placement: None,
            ..record("PWF-0002")
        };
        let store = InMemoryStore::default().with_project("pwf", vec![done, unlinked_active]);
        let projects = projects(&[("pwf", "PWF")]);

        assert_eq!(
            find(&store, &projects, "PWF-0002").unwrap().id.as_ref(),
            "PWF-0002"
        );
        assert_matches!(find(&store, &projects, "PWF-0001"), Err(
            FindActiveTaskError::TaskNotFound { id }
        ) if id.as_ref() == "PWF-0001");
    }

    #[test]
    fn unknown_project_id_reports_the_typed_identifiers() {
        let store = InMemoryStore::default().with_project("pwf", vec![record("PWF-0001")]);
        let projects = projects(&[("pwf", "PWF")]);

        let error = find(&store, &projects, "XYZ-0001").unwrap_err();

        assert!(matches!(
            error,
            FindActiveTaskError::UnknownProjectId {
                ref task_id,
                ref project_id,
            } if task_id.as_ref() == "XYZ-0001" && project_id.as_ref() == "XYZ"
        ));
        assert_eq!(
            error.to_string(),
            "Unknown project ID `XYZ` for task XYZ-0001"
        );
    }
}
