use super::identifier;
use crate::{
    ports::task_record::TaskStore,
    project::{
        ProjectStatusFilter,
        get_active_project::{self, GetActiveProject},
        get_project::GetProjectError,
        list_projects::{self, ListProjects},
    },
    task::{dto::TaskView, logic::finding::find_active_task_in_projects},
};

#[derive(Debug, Clone)]
pub struct FindActiveTask {
    pub id: String,
}

#[derive(Debug, thiserror::Error)]
pub enum FindActiveTaskError {
    #[error("Active task not found: {id}")]
    TaskNotFound { id: String },
    #[error("Task id is ambiguous: {id}")]
    AmbiguousId { id: String },
    #[error("Unknown task id prefix `{prefix}` for {id}")]
    UnknownPrefix { id: String, prefix: String },
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
) -> Result<TaskView, FindActiveTaskError> {
    let projects = match identifier::parse(&query.id) {
        Some(task_id) => match get_active_project::execute(
            GetActiveProject {
                id: task_id.project_id(),
            },
            pool,
        )
        .await
        {
            Ok(project) => vec![project],
            Err(GetProjectError::ProjectNotFound { id: prefix }) => {
                return Err(FindActiveTaskError::UnknownPrefix {
                    id: task_id.to_string(),
                    prefix: prefix.to_string(),
                });
            }
            Err(error) => return Err(FindActiveTaskError::QueryProject(Box::new(error))),
        },
        None => list_projects::execute(
            ListProjects {
                status: ProjectStatusFilter::ACTIVE,
            },
            pool,
        )
        .await
        .map_err(|error| FindActiveTaskError::QueryProject(Box::new(error)))?,
    };
    find_active_task_in_projects(store, &projects, &query.id)
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
        ports::task_record::{IndexPlacement, Materialization, RecordId, TaskRecord},
        task::logic::finding::find_active_task_in_projects,
        testing::{InMemoryStore, project},
    };

    fn record(id: &str) -> TaskRecord {
        TaskRecord {
            id: RecordId::Task(TaskId::try_new(id).unwrap()),
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

    fn inline(ordinal: usize) -> TaskRecord {
        TaskRecord {
            id: RecordId::Inline(ordinal),
            title: "legacy task".to_string(),
            status: TaskStatus::Active,
            created: None,
            completed: None,
            commits: None,
            tags: None,
            effort: None,
            prereq: None,
            section: None,
            body: "do the legacy thing".to_string(),
            source: "do the legacy thing".to_string(),
            locator: "/notes/pwf/pwf.md".to_string(),
            placement: Some(IndexPlacement {
                index_path: "/notes/pwf/pwf.md".to_string(),
                line: ordinal,
            }),
            materialization: Materialization::InlineLegacy,
        }
    }

    fn projects(values: &[(&str, &str)]) -> Vec<Project> {
        values
            .iter()
            .map(|(name, project_id)| project(project_id, name))
            .collect()
    }

    fn find(
        store: &InMemoryStore,
        projects: &[Project],
        id: &str,
    ) -> Result<TaskView, FindActiveTaskError> {
        find_active_task_in_projects(store, projects, id)
    }

    #[test]
    fn finds_open_item_enriched_with_launchability() {
        let store = InMemoryStore::default().with_project("pwf", vec![record("PWF-0001")]);
        let projects = projects(&[("pwf", "PWF")]);

        let task = find(&store, &projects, "PWF-0001").unwrap();

        assert_eq!(task.id, "PWF-0001");
        assert_eq!(task.project, "pwf");
        assert_eq!(task.repo.as_deref(), Some("/work/pwf"));
        assert_eq!(task.prompt, "do the thing");
        assert!(task.launchable);
    }

    #[test]
    fn loose_id_normalization_is_application_owned() {
        let store = InMemoryStore::default().with_project("pwf", vec![record("PWF-0001")]);
        let projects = projects(&[("pwf", "PWF")]);

        assert_eq!(find(&store, &projects, "pwf-0001").unwrap().id, "PWF-0001");
    }

    #[test]
    fn missing_id_errors_not_found_preserving_raw_id() {
        let store = InMemoryStore::default().with_project("pwf", vec![record("PWF-0001")]);
        let projects = projects(&[("pwf", "PWF")]);

        let error = find(&store, &projects, "pwf-9999").unwrap_err();

        assert!(matches!(
            error,
            FindActiveTaskError::TaskNotFound { ref id } if id == "pwf-9999"
        ));
        assert_eq!(error.to_string(), "Active task not found: pwf-9999");
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

        assert_eq!(find(&store, &projects, "PWF-0002").unwrap().id, "PWF-0002");
        assert_matches!(
            find(&store, &projects, "PWF-0001"),
            Err(FindActiveTaskError::TaskNotFound { id }) if id == "PWF-0001"
        );
    }

    #[test]
    fn unknown_prefix_renders_like_the_legacy_adapter() {
        let store = InMemoryStore::default().with_project("pwf", vec![record("PWF-0001")]);
        let projects = projects(&[("pwf", "PWF")]);

        let error = find(&store, &projects, "xyz-0001").unwrap_err();

        assert!(matches!(
            error,
            FindActiveTaskError::UnknownPrefix { ref id, ref prefix }
                if id == "XYZ-0001" && prefix == "XYZ"
        ));
        assert_eq!(
            error.to_string(),
            "Unknown task id prefix `XYZ` for XYZ-0001"
        );
    }

    #[test]
    fn non_canonical_id_resolves_a_legacy_inline_prompt_case_insensitively() {
        let store = InMemoryStore::default().with_project("pwf", vec![inline(1)]);
        let projects = projects(&[("pwf", "PWF")]);

        let task = find(&store, &projects, "PWF:1").unwrap();

        assert_eq!(task.id, "pwf:1");
        assert_eq!(task.format, "legacy");
        assert_eq!(task.session, "legacy task");
    }
}
