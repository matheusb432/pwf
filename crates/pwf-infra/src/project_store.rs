use pwf_models::{
    project::{Project, ProjectCreatedAt, ProjectId, ProjectName},
    task::TaskId,
};

#[derive(Debug)]
pub(super) struct ProjectRow {
    pub(super) id: String,
    pub(super) title: String,
    pub(super) source_kind: Option<String>,
    pub(super) source_value: Option<String>,
    pub(super) tasks_kind: String,
    pub(super) tasks_path: String,
    pub(super) obsidian_vault: Option<String>,
    pub(super) snapshot_enabled: bool,
    pub(super) created_at: String,
    pub(super) is_paused: bool,
}

#[derive(Debug, thiserror::Error)]
#[error("persisted project {field} value {value:?} is invalid: {source}")]
pub(super) struct ProjectRowError {
    pub(super) field: &'static str,
    pub(super) value: String,
    #[source]
    pub(super) source: anyhow::Error,
}

// SQLx checks the complete literal query after this macro expands.
macro_rules! project_query {
    ($suffix:literal $(, $argument:expr)* $(,)?) => {
        ::sqlx::query_as!(
            $crate::project_store::ProjectRow,
            r#"
            SELECT
                projects.id AS "id!",
                projects.title AS "title!",
                project_sources.kind AS "source_kind?",
                project_sources.value AS "source_value?",
                projects.tasks_kind AS "tasks_kind!",
                projects.tasks_path AS "tasks_path!",
                projects.obsidian_vault AS "obsidian_vault?",
                projects.snapshot_enabled AS "snapshot_enabled!: bool",
                projects.created_at AS "created_at!",
                (projects.paused_at IS NOT NULL) AS "is_paused!: bool"
            FROM projects
            LEFT JOIN project_sources ON project_sources.id = projects.project_source_id
            "# + $suffix,
            $($argument),*
        )
    };
}

fn project_from_row(row: ProjectRow) -> Result<Project, ProjectRowError> {
    let id = project_value("id", row.id, ProjectId::try_new)?;
    let title = project_value("title", row.title, ProjectName::try_new)?;
    let source = match (row.source_kind, row.source_value) {
        (None, None) => None,
        (Some(kind), Some(value)) => Some(pwf_models::project::ProjectSource::new(
            project_value("source kind", kind, |value| {
                pwf_models::project::ProjectSourceKind::try_from(value.as_str())
            })?,
            project_value(
                "source value",
                value,
                pwf_models::project::ProjectSourceValue::try_new,
            )?,
        )),
        (kind, value) => {
            return Err(ProjectRowError {
                field: "source",
                value: format!("{kind:?}: {value:?}"),
                source: anyhow::anyhow!("source kind and value must both be present or absent"),
            });
        }
    };
    let tasks_kind = project_value("tasks kind", row.tasks_kind, |value| {
        pwf_models::project::ProjectTasksKind::try_from(value.as_str())
    })?;
    let tasks_path = project_value(
        "tasks path",
        row.tasks_path,
        pwf_models::project::ProjectTasksPath::try_new,
    )?;
    let created_at = project_value("created_at", row.created_at, ProjectCreatedAt::try_new)?;

    Ok(Project {
        id,
        title,
        source,
        tasks: pwf_models::project::ProjectTasks::new(tasks_kind, tasks_path),
        obsidian_vault: row
            .obsidian_vault
            .map(|value| {
                project_value(
                    "obsidian_vault",
                    value,
                    pwf_models::project::ObsidianVault::try_new,
                )
            })
            .transpose()?,
        created_at,
        is_paused: row.is_paused,
        snapshot_enabled: row.snapshot_enabled,
    })
}

fn project_value<T, E>(
    field: &'static str,
    value: String,
    conversion: impl FnOnce(String) -> Result<T, E>,
) -> Result<T, ProjectRowError>
where
    E: std::error::Error + Send + Sync + 'static,
{
    conversion(value.clone()).map_err(|source| ProjectRowError {
        field,
        value,
        source: anyhow::Error::new(source),
    })
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use super::{ProjectRow, project_from_row};

    #[test]
    fn invalid_persisted_project_values_retain_the_model_error() {
        let error = project_from_row(ProjectRow {
            obsidian_vault: None,
            snapshot_enabled: false,
            id: "FOO".to_string(),
            title: "x".repeat(201),
            source_kind: Some("directory".to_string()),
            source_value: Some("/work/foo".to_string()),
            tasks_kind: "directory".to_string(),
            tasks_path: "/tasks/foo".to_string(),
            created_at: "2026-07-25T00:00:00.000Z".to_string(),
            is_paused: false,
        })
        .unwrap_err();

        assert_eq!(error.field, "title");
        assert!(error.source().is_some());
    }
}

mod add_project;
mod get_project;
mod get_projects;
mod list_projects;
mod pause_project;
mod rename_project;
mod resume_project;
mod source_record;
mod update_project;
use std::{collections::BTreeSet, sync::Arc};

use pwf_application::{
    ports::project_store::{ProjectStore, TaskSequenceError},
    project::{
        add_project::AddProjectError, get_project::GetProjectError,
        list_projects::ListProjectsError, pause_project::PauseProjectError,
        rename_project::RenameProjectError, resume_project::ResumeProjectError, task_location,
        update_project::UpdateProjectError,
    },
};
use pwf_models::project::HomeDirectory;
use pwf_wire::project::{
    GetProject, ProjectFields, ProjectStateChange, ProjectStatusFilter, RenameProject,
    UpdateProject,
};
use tokio::sync::Mutex;

mod cache;
mod task_sequence;

#[derive(Clone)]
pub struct SqliteProjectStore {
    pool: sqlx::SqlitePool,
    state: Arc<Mutex<State>>,
}

#[derive(Default)]
struct State {
    connection: Option<sqlx::SqliteConnection>,
    cache: cache::ProjectCache,
    #[cfg(test)]
    reads: usize,
}

impl State {
    async fn connection(
        &mut self,
        pool: &sqlx::SqlitePool,
    ) -> Result<&mut sqlx::SqliteConnection, sqlx::Error> {
        match self.connection {
            Some(ref mut connection) => Ok(connection),
            None => Ok(self.connection.insert(pool.acquire().await?.detach())),
        }
    }
}

impl SqliteProjectStore {
    #[must_use]
    pub fn new(pool: sqlx::SqlitePool) -> Self {
        Self {
            pool,
            state: Arc::default(),
        }
    }
}

impl ProjectStore for SqliteProjectStore {
    async fn reserve_task_id(
        &self,
        project: &ProjectId,
    ) -> Result<Option<TaskId>, TaskSequenceError> {
        let mut state = self.state.lock().await;
        let connection = state
            .connection(&self.pool)
            .await
            .map_err(|source| TaskSequenceError::Unexpected(source.into()))?;
        task_sequence::reserve(project, connection).await
    }

    async fn advance_task_sequence(
        &self,
        project: &ProjectId,
        highest: Option<&TaskId>,
    ) -> Result<(), TaskSequenceError> {
        let mut state = self.state.lock().await;
        let connection = state
            .connection(&self.pool)
            .await
            .map_err(|source| TaskSequenceError::Unexpected(source.into()))?;
        task_sequence::advance(project, highest, connection).await
    }

    async fn get_project(&self, query: GetProject) -> Result<Project, GetProjectError> {
        let mut state = self.state.lock().await;
        if let Some(project) = state.cache.get(&query.id) {
            return if query.status.includes_paused() || !project.is_paused {
                Ok(project)
            } else {
                Err(GetProjectError::ProjectNotFound { id: query.id })
            };
        }
        #[cfg(test)]
        {
            state.reads += 1;
        }
        let connection =
            state
                .connection(&self.pool)
                .await
                .map_err(|source| GetProjectError::Unexpected {
                    context: "opening project store connection",
                    source: source.into(),
                })?;
        let project = get_project::execute(query, connection).await?;
        state.cache.remember(project.clone());
        Ok(project)
    }

    async fn get_projects(
        &self,
        ids: &BTreeSet<ProjectId>,
    ) -> Result<Vec<Project>, GetProjectError> {
        let mut state = self.state.lock().await;
        let mut found = Vec::with_capacity(ids.len());
        let mut missing = BTreeSet::new();
        for id in ids {
            if let Some(project) = state.cache.get(id) {
                found.push(project);
            } else {
                missing.insert(id.clone());
            }
        }
        if !missing.is_empty() {
            #[cfg(test)]
            {
                state.reads += 1;
            }
            let connection = state.connection(&self.pool).await.map_err(|source| {
                GetProjectError::Unexpected {
                    context: "opening project store connection",
                    source: source.into(),
                }
            })?;
            let loaded = get_projects::execute(&missing, connection).await?;
            for project in loaded {
                state.cache.remember(project.clone());
                found.push(project);
            }
        }
        found.sort_unstable_by(|left, right| left.id.cmp(&right.id));
        Ok(found)
    }

    async fn list_projects(
        &self,
        status: ProjectStatusFilter,
    ) -> Result<Vec<Project>, ListProjectsError> {
        let mut state = self.state.lock().await;
        if let Some(projects) = state.cache.list(status) {
            return Ok(projects);
        }
        #[cfg(test)]
        {
            state.reads += 1;
        }
        let connection =
            state
                .connection(&self.pool)
                .await
                .map_err(|source| ListProjectsError::Unexpected {
                    context: "opening project store connection",
                    source: source.into(),
                })?;
        let projects = list_projects::execute(status, connection).await?;
        state.cache.remember_list(status, &projects);
        Ok(projects)
    }
    async fn add_project(
        &self,
        fields: ProjectFields,
        home: &HomeDirectory,
    ) -> Result<Project, AddProjectError> {
        let mut state = self.state.lock().await;
        state.cache.clear();
        let connection =
            state
                .connection(&self.pool)
                .await
                .map_err(|source| AddProjectError::Unexpected {
                    context: "opening project store connection",
                    source: source.into(),
                })?;
        add_project::execute(fields, connection, home).await
    }
    async fn update_project(&self, command: UpdateProject) -> Result<(), UpdateProjectError> {
        let mut state = self.state.lock().await;
        state.cache.clear();
        let connection = state.connection(&self.pool).await.map_err(|source| {
            UpdateProjectError::Unexpected {
                context: "opening project store connection",
                source: source.into(),
            }
        })?;
        update_project::execute(command, connection).await
    }
    async fn pause_project(
        &self,
        project_id: ProjectId,
    ) -> Result<ProjectStateChange, PauseProjectError> {
        let mut state = self.state.lock().await;
        state.cache.clear();
        let connection =
            state
                .connection(&self.pool)
                .await
                .map_err(|source| PauseProjectError::Unexpected {
                    context: "opening project store connection",
                    source: source.into(),
                })?;
        pause_project::execute(project_id, connection).await
    }
    async fn resume_project(
        &self,
        project_id: ProjectId,
        home: &HomeDirectory,
    ) -> Result<ProjectStateChange, ResumeProjectError> {
        let mut state = self.state.lock().await;
        state.cache.clear();
        let connection = state.connection(&self.pool).await.map_err(|source| {
            ResumeProjectError::Unexpected {
                context: "opening project store connection",
                source: source.into(),
            }
        })?;
        resume_project::execute(project_id, connection, home).await
    }
    async fn rename_project(
        &self,
        command: RenameProject,
        expected: &Project,
        home: &HomeDirectory,
    ) -> Result<Project, RenameProjectError> {
        let mut state = self.state.lock().await;
        state.cache.clear();
        let connection = state.connection(&self.pool).await.map_err(|source| {
            RenameProjectError::Unexpected {
                context: "opening project store connection",
                source: source.into(),
            }
        })?;
        rename_project::execute(command, expected, connection, home).await
    }
}

#[cfg(test)]
mod contract;
