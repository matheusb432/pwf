use std::path::{Path, PathBuf};

use pwf_models::task::TaskId;
use serde::Deserialize;

use super::{FrontmatterView, MarkdownFile, MarkdownFileError, ObsidianStoreError};

/// Contains a task note discovered by frontmatter identity.
pub struct TaskNoteIdentity {
    pub id: TaskId,
    pub path: PathBuf,
    pub markdown: String,
    pub title: Option<String>,
}

/// Inventories one project directory without deriving task identity from filenames.
pub fn inspect_project_task_notes(
    project_dir: &Path,
    project_page_path: &Path,
) -> Result<Vec<TaskNoteIdentity>, ObsidianStoreError> {
    map_project_task_notes(
        project_dir,
        project_page_path,
        MarkdownFile::read_source,
        |id, title, _, _| Ok((id, title)),
    )
    .map(|notes| {
        notes
            .into_iter()
            .map(|((id, title), file)| {
                let (path, markdown) = file.into_parts();
                TaskNoteIdentity {
                    id,
                    path,
                    markdown,
                    title,
                }
            })
            .collect()
    })
}

pub(super) fn map_project_task_notes<T>(
    project_dir: &Path,
    project_page_path: &Path,
    read: fn(&Path) -> Result<MarkdownFile, MarkdownFileError>,
    map: impl Fn(
        TaskId,
        Option<String>,
        &MarkdownFile,
        &FrontmatterView<'_>,
    ) -> Result<T, ObsidianStoreError>,
) -> Result<Vec<(T, MarkdownFile)>, ObsidianStoreError> {
    let mut tasks = Vec::new();
    for entry in std::fs::read_dir(project_dir)
        .map_err(|source| ObsidianStoreError::ReadTaskFile { source })?
    {
        let entry = entry.map_err(|source| ObsidianStoreError::ReadTaskFile { source })?;
        let path = entry.path();
        if path == project_page_path
            || path
                .file_name()
                .is_some_and(|name| name == super::PROJECT_SNAPSHOT_FILE_NAME)
            || path.extension().and_then(|extension| extension.to_str()) != Some("md")
        {
            continue;
        }
        let file = read(&path).map_err(|source| ObsidianStoreError::ReadTaskFile {
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
