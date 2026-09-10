use std::path::{Path, PathBuf};

use pwf_models::project::{
    HomeDirectory, ObsidianVault, ProjectId, ProjectName, ProjectSource, ProjectSourceKind,
    ProjectTasks, ProjectTasksKind, ProjectTasksPath,
};
use pwf_wire::project::{AddVaultProject, ProjectFields};

use super::add_project::{self, AddProjectError};
use crate::ports::project_directory::ProjectDirectoryClient;

#[derive(Debug, thiserror::Error)]
pub enum AddVaultProjectError {
    #[error(transparent)]
    AddProject(#[from] AddProjectError),
    #[error("vault path must be absolute or home-relative: {path}")]
    RelativeVault { path: PathBuf },
    #[error("resolving vault {path}: {source}")]
    ResolveVault {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("vault has no .obsidian folder: {path}")]
    NotVault { path: PathBuf },
    #[error("vault path is not valid unicode: {path}")]
    NonUnicode { path: PathBuf },
    #[error(transparent)]
    Title(#[from] pwf_models::project::ProjectNameError),
}

#[cqrsy::command]
pub async fn execute(
    command: AddVaultProject,
    pool: &sqlx::SqlitePool,
    home: &HomeDirectory,
    directory: &impl ProjectDirectoryClient,
) -> Result<ProjectId, AddVaultProjectError> {
    let raw = command.vault_path.as_ref();
    let path = if raw == "~" {
        home.as_path().to_path_buf()
    } else {
        raw.strip_prefix("~/")
            .or_else(|| raw.strip_prefix("~\\"))
            .map_or_else(
                || PathBuf::from(raw),
                |relative| home.as_path().join(relative),
            )
    };
    if !path.is_absolute() {
        return Err(AddVaultProjectError::RelativeVault { path });
    }
    let vault = directory
        .canonicalize(&path)
        .map_err(|source| AddVaultProjectError::ResolveVault { path, source })?;
    if !directory.is_directory(&vault.join(".obsidian")) {
        return Err(AddVaultProjectError::NotVault { path: vault });
    }
    let relative = Path::new(command.tasks_path.as_ref());
    let title = command.title.map_or_else(
        || {
            let raw = command.tasks_path.as_ref();
            ProjectName::try_new(raw.rsplit_once(['/', '\\']).map_or(raw, |(_, title)| title))
        },
        Ok,
    )?;
    let tasks = vault.join(relative);
    let tasks = tasks
        .to_str()
        .ok_or_else(|| AddVaultProjectError::NonUnicode {
            path: tasks.clone(),
        })?;
    let vault = vault
        .to_str()
        .ok_or_else(|| AddVaultProjectError::NonUnicode {
            path: vault.clone(),
        })?;
    let fields = ProjectFields {
        snapshot_enabled: false,
        id: command.id,
        title,
        source: command
            .source_path
            .map(|value| ProjectSource::new(ProjectSourceKind::Directory, value)),
        tasks: ProjectTasks::new(
            ProjectTasksKind::Directory,
            ProjectTasksPath::try_new(tasks).map_err(|source| AddProjectError::Unexpected {
                context: "converting vault tasks path",
                source: source.into(),
            })?,
        ),
        obsidian_vault: Some(ObsidianVault::try_new(vault).map_err(|source| {
            AddProjectError::Unexpected {
                context: "converting vault path",
                source: source.into(),
            }
        })?),
    };
    add_project::execute(fields, pool, home)
        .await
        .map(|project| project.id)
        .map_err(Into::into)
}
