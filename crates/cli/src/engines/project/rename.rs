use std::path::{Path, PathBuf};

use clap::Args;
use pwf_application::project::{
    Project,
    add_project::AddProjectFields,
    get_project::{self, GetProject},
    rename_project::{self, RenameProject},
    resolve_runtime_path::{self, ResolveRuntimePath},
};
use pwf_domain::project::{
    ProjectIndexIdentity, ProjectName, ProjectPrefix, ProjectSource, ProjectSourceKind,
    ProjectSourceValue, ProjectTasks, ProjectTasksKind, ProjectTasksPath,
};
use pwf_infra::{
    SqliteStore,
    obsidian::project_rename::{self, ProjectRenameCommit, StagedProjectRename},
};

use super::{output, parse_project_id};

#[derive(Args, Debug)]
pub struct Arguments {
    /// Existing canonical project ID.
    #[arg(value_parser = parse_project_id)]
    pub current_id: ProjectPrefix,
    /// Replacement canonical project ID.
    #[arg(value_parser = parse_project_id)]
    pub destination_id: ProjectPrefix,
    /// Replacement project title.
    #[arg(long, value_parser = parse_project_title)]
    pub title: ProjectName,
    /// Replacement project source directory.
    #[arg(long, value_parser = parse_project_source)]
    pub source: ProjectSourceValue,
    /// Replacement pending-work task directory.
    #[arg(long, value_parser = parse_project_tasks)]
    pub tasks: ProjectTasksPath,
}

pub(super) async fn run(
    arguments: Arguments,
    database: &SqliteStore,
    home: PathBuf,
) -> Result<String, String> {
    let current = get_project::execute(
        GetProject {
            id: arguments.current_id.clone(),
        },
        database,
    )
    .await
    .map_err(|error| format!("project rename source lookup failed: {error}"))?;
    let fields = AddProjectFields {
        id: arguments.destination_id,
        title: arguments.title,
        source: ProjectSource::new(ProjectSourceKind::Directory, arguments.source),
        tasks: ProjectTasks::new(ProjectTasksKind::Directory, arguments.tasks),
    };
    let source_tasks = resolve_tasks_path(&current.id, &current.tasks, &home)?;
    let destination_tasks = resolve_tasks_path(&fields.id, &fields.tasks, &home)?;
    let current_identity = ProjectIndexIdentity::new(current.id.clone(), current.title.clone());
    let next_identity = ProjectIndexIdentity::new(fields.id.clone(), fields.title.clone());
    let staged = project_rename::stage(
        &source_tasks,
        &destination_tasks,
        &current_identity,
        &next_identity,
    )
    .map_err(|error| format!("project rename staging failed: {error}"))?;
    let renamed = match rename_project::execute(
        RenameProject {
            current_id: current.id.clone(),
            fields,
            home: home.clone(),
        },
        database,
    )
    .await
    {
        Ok(renamed) => renamed,
        Err(error) => return application_failure(staged, &error),
    };

    match staged.commit() {
        Ok(ProjectRenameCommit::Complete) => output::project(renamed),
        Ok(ProjectRenameCommit::BackupRetained { path, source }) => Err(format!(
            "project rename committed, but removing filesystem backup {} failed: {source}; registry remains renamed",
            path.display()
        )),
        Err(commit_error) => {
            let rollback = rename_project::execute(
                RenameProject {
                    current_id: renamed.id.clone(),
                    fields: project_fields(&current),
                    home,
                },
                database,
            )
            .await;
            match rollback {
                Ok(_) => Err(format!(
                    "project rename filesystem commit failed: {commit_error}; registry rollback succeeded"
                )),
                Err(rollback_error) => Err(format!(
                    "project rename filesystem commit failed: {commit_error}; registry rollback failed: {rollback_error}"
                )),
            }
        }
    }
}

fn application_failure(
    staged: StagedProjectRename,
    error: &rename_project::RenameProjectError,
) -> Result<String, String> {
    match staged.discard() {
        Ok(()) => Err(format!("project rename failed: {error}")),
        Err(discard_error) => Err(format!(
            "project rename failed: {error}; removing staging directory failed: {discard_error}"
        )),
    }
}

fn resolve_tasks_path(
    project_id: &ProjectPrefix,
    tasks: &ProjectTasks,
    home: &Path,
) -> Result<PathBuf, String> {
    resolve_runtime_path::execute(&ResolveRuntimePath {
        path: tasks.path().as_ref().to_string(),
        home: home.to_path_buf(),
    })
    .map(|resolved| resolved.path().to_path_buf())
    .map_err(|error| {
        format!(
            "managed project {project_id} task path '{}' is invalid: {error}",
            tasks.path()
        )
    })
}

fn project_fields(project: &Project) -> AddProjectFields {
    AddProjectFields {
        id: project.id.clone(),
        title: project.title.clone(),
        source: project.source.clone(),
        tasks: project.tasks.clone(),
    }
}

fn parse_project_title(raw: &str) -> Result<ProjectName, String> {
    ProjectName::try_new(raw.to_string()).map_err(|_| {
        if raw.trim().eq_ignore_ascii_case("project") {
            "project title is reserved".to_string()
        } else {
            "project title must not be blank".to_string()
        }
    })
}

fn parse_project_source(raw: &str) -> Result<ProjectSourceValue, String> {
    ProjectSourceValue::try_new(raw.to_string())
        .map_err(|_| "project source value must not be blank".to_string())
}

fn parse_project_tasks(raw: &str) -> Result<ProjectTasksPath, String> {
    ProjectTasksPath::try_new(raw.to_string())
        .map_err(|_| "project tasks path must not be blank".to_string())
}
