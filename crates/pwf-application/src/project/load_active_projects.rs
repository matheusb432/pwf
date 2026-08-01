use std::path::{Path, PathBuf};

use pwf_models::project::ProjectId;

use super::{
    Project,
    list_projects::{self, ListProjects, ListProjectsError},
    logic::task_location::{self, TaskLocationError},
    resolve_runtime_path::{self, ResolveRuntimePath, RuntimePathError},
};

/// Requests active projects with paths resolved for the current process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadActiveProjects {
    /// Home directory used to expand home-relative project paths.
    pub home: PathBuf,
}

/// One active project prepared for runtime adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveProject {
    /// Persisted project data.
    pub project: Project,
    /// Resolved source location.
    pub source_path: PathBuf,
    /// Resolved pending-work task location.
    pub tasks_path: PathBuf,
}

#[derive(Debug, thiserror::Error)]
pub enum LoadActiveProjectsError {
    #[error("listing active projects failed: {0}")]
    List(#[from] ListProjectsError),
    #[error("managed project {project_id} {field} path '{path}' is invalid: {source}")]
    InvalidPath {
        project_id: ProjectId,
        field: &'static str,
        path: String,
        #[source]
        source: RuntimePathError,
    },
    #[error(
        "managed projects {first_id} and {second_id} resolve to the same task location: {}",
        path.display()
    )]
    DuplicateTaskLocation {
        first_id: ProjectId,
        second_id: ProjectId,
        path: PathBuf,
    },
}

/// Loads active projects and resolves their paths for runtime adapters.
///
/// # Errors
///
/// Returns [`LoadActiveProjectsError`] when persisted projects cannot be read, a path is invalid,
/// or active projects resolve to the same task location.
#[cqrsy::query]
pub async fn execute(
    query: LoadActiveProjects,
    pool: &sqlx::SqlitePool,
) -> Result<Vec<ActiveProject>, LoadActiveProjectsError> {
    let projects = list_projects::execute(
        ListProjects {
            include_paused: false,
        },
        pool,
    )
    .await?;

    resolve_projects(projects, &query.home)
}

fn resolve_projects(
    projects: Vec<Project>,
    home: &Path,
) -> Result<Vec<ActiveProject>, LoadActiveProjectsError> {
    let mut task_path_owners = Vec::with_capacity(projects.len());
    let mut active = Vec::with_capacity(projects.len());

    for project in projects {
        let source_path =
            resolve_path(&project.id, "source", project.source.value().as_ref(), home)?;
        let tasks_path = task_location::reject_collision(
            &project.id,
            project.tasks.path().as_ref(),
            task_path_owners.iter().cloned(),
            home,
        )
        .map_err(task_location_error)?;
        task_path_owners.push((
            project.id.clone(),
            project.tasks.path().as_ref().to_string(),
        ));
        active.push(ActiveProject {
            project,
            source_path: source_path.path().to_path_buf(),
            tasks_path: tasks_path.path().to_path_buf(),
        });
    }

    Ok(active)
}

fn resolve_path(
    project_id: &ProjectId,
    field: &'static str,
    path: &str,
    home: &Path,
) -> Result<resolve_runtime_path::ResolvedPath, LoadActiveProjectsError> {
    resolve_runtime_path::execute(&ResolveRuntimePath {
        path: path.to_string(),
        home: home.to_path_buf(),
    })
    .map_err(|source| LoadActiveProjectsError::InvalidPath {
        project_id: project_id.clone(),
        field,
        path: path.to_string(),
        source,
    })
}

fn task_location_error(error: TaskLocationError) -> LoadActiveProjectsError {
    match error {
        TaskLocationError::InvalidPath {
            project_id,
            path,
            source,
        } => LoadActiveProjectsError::InvalidPath {
            project_id,
            field: "task",
            path,
            source,
        },
        TaskLocationError::Collision {
            first_id,
            second_id,
            path,
        } => LoadActiveProjectsError::DuplicateTaskLocation {
            first_id,
            second_id,
            path,
        },
    }
}

#[cfg(test)]
mod tests {
    use pwf_models::project::{
        ProjectName, ProjectSource, ProjectSourceKind, ProjectSourceValue, ProjectTasks,
        ProjectTasksKind, ProjectTasksPath,
    };

    use super::*;

    fn project(id: &str, title: &str, source: &str, tasks: &str) -> Project {
        Project {
            id: ProjectId::try_new(id).unwrap(),
            title: ProjectName::try_new(title).unwrap(),
            source: ProjectSource::new(
                ProjectSourceKind::Directory,
                ProjectSourceValue::try_new(source).unwrap(),
            ),
            tasks: ProjectTasks::new(
                ProjectTasksKind::Directory,
                ProjectTasksPath::try_new(tasks).unwrap(),
            ),
            created_at: "2026-07-25T00:00:00.000Z".to_string(),
            is_paused: false,
        }
    }

    #[test]
    fn runtime_task_aliases_are_rejected_with_stable_project_order() {
        let projects = vec![
            project("PWF", "pwf", "/work/pwf", "~/tasks/shared"),
            project("ALT", "other", "/work/other", "/home/tester/tasks/shared"),
        ];

        let error = resolve_projects(projects, Path::new("/home/tester")).unwrap_err();

        assert_eq!(
            error.to_string(),
            "managed projects ALT and PWF resolve to the same task location: \
             /home/tester/tasks/shared"
        );
    }

    #[test]
    fn home_relative_paths_are_resolved_for_runtime_use() {
        let projects = vec![project("PWF", "pwf", "~/tools/pwf", "~/pending-work/pwf")];

        let active = resolve_projects(projects, Path::new("/home/tester")).unwrap();

        assert_eq!(active.len(), 1);
        assert_eq!(
            active[0].source_path,
            PathBuf::from("/home/tester/tools/pwf")
        );
        assert_eq!(
            active[0].tasks_path,
            PathBuf::from("/home/tester/pending-work/pwf")
        );
    }
}
