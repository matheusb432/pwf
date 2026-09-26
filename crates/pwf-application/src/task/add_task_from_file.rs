use std::{io, path::PathBuf};

use pwf_models::task::{TaskBody, TaskId, TaskTitle, TaskTitleError};
use pwf_wire::task::{AddTask, AddTaskBody, AddTaskFromFile, TaskMutationResult};

use super::add_task::{self, AddTaskError};
use crate::ports::{
    clock::Clock, project_store::ProjectStore, task_source_file::TaskSourceFileReader,
    task_vault::TaskVault, user_settings::TaskBodyPresetReader,
};

#[derive(Debug, thiserror::Error)]
pub enum AddTaskFromFileError {
    #[error("source file path {path} has no UTF-8 filename stem")]
    InvalidSourceFileName { path: PathBuf },
    #[error(transparent)]
    InvalidTitle(#[from] TaskTitleError),
    #[error("cannot read task source file {path}: {source}")]
    ReadSourceFile {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error(transparent)]
    AddTask(#[from] AddTaskError),
}

#[cqrsy::command]
pub async fn execute(
    command: AddTaskFromFile,
    source_files: &impl TaskSourceFileReader,
    store: &impl TaskVault,
    project_store: &impl ProjectStore,
    clock: &impl Clock,
    preset_reader: &impl TaskBodyPresetReader,
) -> Result<TaskMutationResult<TaskId>, AddTaskFromFileError> {
    let AddTaskFromFile {
        project_id,
        source_file,
        title,
    } = command;
    let title = match title {
        Some(title) => title,
        None => title_from_source_file(&source_file)?,
    };
    let body = source_files
        .read_task_source_file(&source_file)
        .map_err(|source| AddTaskFromFileError::ReadSourceFile {
            path: source_file,
            source,
        })?;

    add_task::execute(
        AddTask::new(
            project_id,
            AddTaskBody::from_body(title, TaskBody::new(body)),
        ),
        store,
        project_store,
        clock,
        preset_reader,
    )
    .await
    .map_err(Into::into)
}

fn title_from_source_file(path: &std::path::Path) -> Result<TaskTitle, AddTaskFromFileError> {
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| AddTaskFromFileError::InvalidSourceFileName {
            path: path.to_path_buf(),
        })?;
    TaskTitle::try_new(stem).map_err(Into::into)
}
