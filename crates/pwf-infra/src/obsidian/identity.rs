use std::path::{Path, PathBuf};

use pwf_models::{
    project::{ProjectId, ProjectIndexIdentity, ProjectName},
    task::TaskId,
};
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
    index_path: &Path,
    expected_identity: &ProjectIndexIdentity,
) -> Result<Vec<TaskNoteIdentity>, ObsidianStoreError> {
    map_project_task_notes(
        project_dir,
        index_path,
        expected_identity,
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
    index_path: &Path,
    expected_identity: &ProjectIndexIdentity,
    read: fn(&Path) -> Result<MarkdownFile, MarkdownFileError>,
    map: impl Fn(
        TaskId,
        Option<String>,
        &MarkdownFile,
        &FrontmatterView<'_>,
    ) -> Result<T, ObsidianStoreError>,
) -> Result<Vec<(T, MarkdownFile)>, ObsidianStoreError> {
    if index_path.exists() {
        let index_file =
            MarkdownFile::open(index_path).map_err(|source| ObsidianStoreError::ReadIndex {
                source: source.into_io_error(),
            })?;
        let actual = parse_project_index_identity(&index_file)?;
        validate_project_index_identity(index_path, &actual, expected_identity)?;
    }

    let mut tasks = Vec::new();
    for entry in std::fs::read_dir(project_dir)
        .map_err(|source| ObsidianStoreError::ReadTaskFile { source })?
    {
        let entry = entry.map_err(|source| ObsidianStoreError::ReadTaskFile { source })?;
        let path = entry.path();
        if path == index_path
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

#[derive(Deserialize)]
struct ProjectIndexFrontmatter {
    id: Option<String>,
    title: Option<String>,
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

fn parse_task_metadata(
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

pub(super) fn parse_project_index_identity(
    file: &MarkdownFile,
) -> Result<ProjectIndexIdentity, ObsidianStoreError> {
    let path = file.path();
    let frontmatter = parse_frontmatter::<ProjectIndexFrontmatter>(file, "id/title")?;
    let raw_id = required_index_property(path, "id", frontmatter.id)?;
    let raw_title = required_index_property(path, "title", frontmatter.title)?;
    let id = ProjectId::try_new(&raw_id).map_err(|_| {
        ObsidianStoreError::InvalidProjectIndexProperty {
            path: path.to_path_buf(),
            property: "id",
            value: raw_id,
        }
    })?;
    let title = ProjectName::try_new(&raw_title).map_err(|_| {
        ObsidianStoreError::InvalidProjectIndexProperty {
            path: path.to_path_buf(),
            property: "title",
            value: raw_title,
        }
    })?;
    Ok(ProjectIndexIdentity::new(id, title))
}

pub(super) fn validate_project_index_identity(
    path: &Path,
    actual: &ProjectIndexIdentity,
    expected: &ProjectIndexIdentity,
) -> Result<(), ObsidianStoreError> {
    if actual == expected {
        return Ok(());
    }
    Err(ObsidianStoreError::ProjectIndexIdentityMismatch {
        path: path.to_path_buf(),
        actual_id: actual.id().clone(),
        actual_title: actual.title().clone(),
        expected_id: expected.id().clone(),
        expected_title: expected.title().clone(),
    })
}

pub(super) fn new_project_index_content(identity: &ProjectIndexIdentity) -> String {
    format!(
        "---\nid: {}\ntitle: {}\n---\n\n",
        project_index_frontmatter_id(identity),
        identity.title()
    )
}

pub(super) fn project_index_frontmatter_id(identity: &ProjectIndexIdentity) -> String {
    identity.id().as_ref().to_ascii_lowercase()
}

fn parse_frontmatter<T: serde::de::DeserializeOwned>(
    file: &MarkdownFile,
    property: &'static str,
) -> Result<T, ObsidianStoreError> {
    file.frontmatter::<T>()
        .map_err(|source| ObsidianStoreError::FrontmatterParse {
            path: file.path().to_path_buf(),
            property,
            source,
        })?
        .ok_or_else(|| ObsidianStoreError::MissingFrontmatter {
            path: file.path().to_path_buf(),
            property,
        })
}

fn required_index_property(
    path: &Path,
    property: &'static str,
    value: Option<String>,
) -> Result<String, ObsidianStoreError> {
    value.ok_or_else(|| ObsidianStoreError::MissingProjectIndexProperty {
        path: PathBuf::from(path),
        property,
    })
}

#[cfg(test)]
mod tests {
    use std::{assert_matches, path::Path};

    use pwf_models::project::{ProjectId, ProjectIndexIdentity, ProjectName};

    use super::{
        parse_project_index_identity, parse_task_metadata, project_index_frontmatter_id,
        validate_project_index_identity,
    };
    use crate::obsidian::ObsidianStoreError;

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

    #[test]
    fn parses_project_index_identity_into_domain_types() {
        let path = Path::new("/vault/sample-project/index.md");
        let markdown = "---\nid: smp\ntitle: sample-project\n---\n\n# Tasks\n";
        let file = crate::obsidian::MarkdownFile::from_source(path, markdown.to_string());

        let identity = parse_project_index_identity(&file).unwrap();

        assert_eq!(identity.id().as_ref(), "SMP");
        assert_eq!(project_index_frontmatter_id(&identity), "smp");
        assert_eq!(identity.title().as_ref(), "sample-project");
    }

    #[test]
    fn rejects_project_index_identity_that_disagrees_with_supplied_identity() {
        let path = Path::new("/vault/sample-project/index.md");
        let actual = ProjectIndexIdentity::new(
            ProjectId::try_new("foo").unwrap(),
            ProjectName::try_new("foo").unwrap(),
        );
        let expected = ProjectIndexIdentity::new(
            ProjectId::try_new("smp").unwrap(),
            ProjectName::try_new("sample-project").unwrap(),
        );

        let error = validate_project_index_identity(path, &actual, &expected).unwrap_err();

        assert_matches!(
            error,
            ObsidianStoreError::ProjectIndexIdentityMismatch { path: ref actual, .. }
                if actual == path
        );
    }
}
