use std::{io, path::PathBuf};

use pwf_models::task::{TaskBody, TaskId, TaskTitle, TaskTitleError};
use pwf_wire::task::{AddTask, AddTaskBody, AddTaskFromFile, TaskMutationResult};

use super::add_task::{self, AddTaskError};
use crate::ports::{
    clock::Clock, project_store::ProjectStore, task_marker_section_store::TaskMarkerSectionStore,
    task_source_file::TaskSourceFileReader, task_vault::TaskVault,
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
    marker_section_store: &impl TaskMarkerSectionStore,
) -> Result<TaskMutationResult<TaskId>, AddTaskFromFileError> {
    let AddTaskFromFile {
        project_id,
        source_file,
    } = command;
    let title = title_from_source_file(&source_file)?;
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
        marker_section_store,
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
