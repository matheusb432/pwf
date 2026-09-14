use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use pwf_application::ports::task_vault::{TaskGraphRecord, TaskGraphSnapshot, TaskSummaryRecord};
use pwf_models::task::TaskId;
use serde::Deserialize;

use super::{
    FrontmatterView, MarkdownFile, MarkdownFileError, ObsidianStoreError,
    project_snapshot_backup_path, project_snapshot_path,
};

#[cfg(test)]
thread_local! {
    static TASK_DIRECTORY_SCAN_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn task_directory_scan_count() -> usize {
    TASK_DIRECTORY_SCAN_COUNT.get()
}

/// Contains a task file's discovered identity and path.
pub(super) struct TaskFileIdentity {
    pub(super) id: TaskId,
    pub(super) path: PathBuf,
}

pub(super) struct TaskFile {
    pub(super) id: TaskId,
    pub(super) path: PathBuf,
    pub(super) summary: Option<Box<TaskSummaryRecord>>,
    pub(super) graph: Result<Arc<TaskGraphRecord>, Arc<ObsidianStoreError>>,
}

pub struct TaskGraphFiles {
    pub(super) files: Arc<[TaskFile]>,
}

impl TaskGraphSnapshot for TaskGraphFiles {
    type Error = ObsidianStoreError;

    fn get(&self, id: &TaskId) -> Option<&Result<Arc<TaskGraphRecord>, Arc<Self::Error>>> {
        self.files
            .binary_search_by(|file| file.id.cmp(id))
            .ok()
            .map(|position| &self.files[position].graph)
    }

    fn iter(
        &self,
    ) -> impl Iterator<Item = (&TaskId, &Result<Arc<TaskGraphRecord>, Arc<Self::Error>>)> {
        self.files.iter().map(|file| (&file.id, &file.graph))
    }
}

#[derive(Clone, Copy)]
pub(super) enum TaskRead {
    Frontmatter,
    Source,
}

impl TaskRead {
    fn read(self, path: &Path) -> Result<MarkdownFile, MarkdownFileError> {
        match self {
            Self::Frontmatter => MarkdownFile::read_frontmatter_file(path),
            Self::Source => MarkdownFile::read_source(path),
        }
    }
}

pub(super) fn map_project_task_files<T>(
    project_dir: &Path,
    read: TaskRead,
    map: impl Fn(
        TaskId,
        Option<String>,
        &MarkdownFile,
        &FrontmatterView<'_>,
    ) -> Result<T, ObsidianStoreError>,
) -> Result<Vec<(T, MarkdownFile)>, ObsidianStoreError> {
    #[cfg(test)]
    TASK_DIRECTORY_SCAN_COUNT.set(TASK_DIRECTORY_SCAN_COUNT.get() + 1);
    let mut tasks = Vec::new();
    let excluded_paths = [
        project_snapshot_path(project_dir),
        project_snapshot_backup_path(project_dir),
    ];
    for entry in std::fs::read_dir(project_dir)
        .map_err(|source| ObsidianStoreError::ReadTaskFile { source })?
    {
        let entry = entry.map_err(|source| ObsidianStoreError::ReadTaskFile { source })?;
        let path = entry.path();
        if excluded_paths
            .iter()
            .flatten()
            .any(|excluded| path == *excluded)
            || path.extension().and_then(|extension| extension.to_str()) != Some("md")
        {
            continue;
        }
        let file = read
            .read(&path)
            .map_err(|source| ObsidianStoreError::ReadTaskFile {
                source: source.into_io_error(),
            })?;
        let Some(frontmatter) = task_frontmatter(&file)? else {
            continue;
        };
        let Some((id, title)) = parse_task_metadata(&path, &frontmatter)? else {
            continue;
        };
        let record = map(id.clone(), title, &file, &frontmatter);
        drop(frontmatter);
        tasks.push((id, path, record.map(|metadata| (metadata, file))));
    }
    tasks.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    for pair in tasks.windows(2) {
        if pair[0].0 == pair[1].0 {
            return Err(ObsidianStoreError::DuplicateTaskId {
                id: pair[0].0.clone(),
                paths: vec![pair[0].1.clone(), pair[1].1.clone()],
            });
        }
    }
    tasks.into_iter().map(|(_, _, record)| record).collect()
}

#[derive(Deserialize)]
struct TaskFrontmatter {
    id: Option<String>,
    title: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
}

fn task_frontmatter(
    file: &MarkdownFile,
) -> Result<Option<FrontmatterView<'_>>, ObsidianStoreError> {
    file.frontmatter_view()
        .map_err(|source| ObsidianStoreError::FrontmatterParse {
            path: file.path().to_path_buf(),
            property: "id",
            source,
        })
}

pub(super) fn parse_task_metadata(
    path: &Path,
    frontmatter: &FrontmatterView<'_>,
) -> Result<Option<(TaskId, Option<String>)>, ObsidianStoreError> {
    let Some(raw_id) =
        frontmatter
            .get("id")
            .map_err(|source| ObsidianStoreError::FrontmatterParse {
                path: path.to_path_buf(),
                property: "id",
                source,
            })?
    else {
        return Ok(None);
    };
    let task = frontmatter
        .deserialize::<TaskFrontmatter>()
        .map_err(|source| ObsidianStoreError::FrontmatterParse {
            path: path.to_path_buf(),
            property: "id",
            source,
        })?;
    if task.kind.as_deref() == Some("note") {
        return Ok(None);
    }
    let raw = task.id.ok_or_else(|| ObsidianStoreError::InvalidTaskId {
        path: path.to_path_buf(),
        value: raw_id.to_string(),
    })?;
    TaskId::try_new(&raw)
        .map(|id| Some((id, task.title)))
        .map_err(|_| ObsidianStoreError::InvalidTaskId {
            path: path.to_path_buf(),
            value: raw,
        })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::parse_task_metadata;

    #[test]
    fn parses_task_id_independently_of_filename() {
        let path = Path::new("/vault/foo/descriptive-name.md");
        let markdown = "---\nid: FOO-0001\nstatus: active\n---\n\nbody\n";
        let file = crate::obsidian::MarkdownFile::from_source(path, markdown.to_string());

        let (id, _) = parse_task_metadata(path, &file.frontmatter_view().unwrap().unwrap())
            .unwrap()
            .unwrap();

        assert_eq!(id.as_ref(), "FOO-0001");
    }
}
