use std::path::{Path, PathBuf};

use gray_matter::{Matter, engine::YAML};
use pwf_models::{
    project::{ProjectId, ProjectIndexIdentity, ProjectName},
    task::TaskId,
};
use serde::Deserialize;

use super::ObsidianStoreError;

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
    if index_path.exists() {
        let index_markdown = std::fs::read_to_string(index_path)
            .map_err(|source| ObsidianStoreError::ReadIndex { source })?;
        let actual = parse_project_index_identity(index_path, &index_markdown)?;
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
        let markdown = std::fs::read_to_string(&path)
            .map_err(|source| ObsidianStoreError::ReadTaskFile { source })?;
        let Some((id, title)) = parse_task_metadata_if_task(&path, &markdown)? else {
            continue;
        };
        tasks.push(TaskNoteIdentity {
            id,
            path,
            markdown,
            title,
        });
    }
    tasks.sort_by(|left, right| {
        left.id
            .cmp(&right.id)
            .then_with(|| left.path.cmp(&right.path))
    });
    for pair in tasks.windows(2) {
        if pair[0].id == pair[1].id {
            return Err(ObsidianStoreError::DuplicateTaskId {
                id: pair[0].id.clone(),
                paths: vec![pair[0].path.clone(), pair[1].path.clone()],
            });
        }
    }
    Ok(tasks)
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

fn parse_task_metadata_if_task(
    path: &Path,
    markdown: &str,
) -> Result<Option<(TaskId, Option<String>)>, ObsidianStoreError> {
    let frontmatter = parse_frontmatter::<TaskFrontmatter>(path, markdown, "id")?;
    if frontmatter.kind.as_deref() == Some("note") {
        return Ok(None);
    }
    let raw = frontmatter
        .id
        .ok_or_else(|| ObsidianStoreError::MissingTaskId {
            path: path.to_path_buf(),
        })?;
    TaskId::try_new(&raw)
        .map(|id| Some((id, frontmatter.title)))
        .map_err(|_| ObsidianStoreError::InvalidTaskId {
            path: path.to_path_buf(),
            value: raw,
        })
}

pub(super) fn parse_project_index_identity(
    path: &Path,
    markdown: &str,
) -> Result<ProjectIndexIdentity, ObsidianStoreError> {
    let frontmatter = parse_frontmatter::<ProjectIndexFrontmatter>(path, markdown, "id/title")?;
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
    path: &Path,
    markdown: &str,
    property: &'static str,
) -> Result<T, ObsidianStoreError> {
    Matter::<YAML>::new()
        .parse::<T>(markdown.strip_prefix('\u{feff}').unwrap_or(markdown))
        .map_err(|source| ObsidianStoreError::FrontmatterParse {
            path: path.to_path_buf(),
            property,
            source,
        })?
        .data
        .ok_or_else(|| ObsidianStoreError::MissingFrontmatter {
            path: path.to_path_buf(),
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
        parse_project_index_identity, parse_task_metadata_if_task, project_index_frontmatter_id,
        validate_project_index_identity,
    };
    use crate::obsidian::ObsidianStoreError;

    #[test]
    fn parses_task_id_independently_of_filename() {
        let path = Path::new("/vault/pwf/descriptive-name.md");
        let markdown = "---\nid: PWF-0124\nstatus: active\n---\n\nbody\n";

        let (id, _) = parse_task_metadata_if_task(path, markdown)
            .unwrap()
            .unwrap();

        assert_eq!(id.as_ref(), "PWF-0124");
    }

    #[test]
    fn missing_task_id_is_path_specific_corruption() {
        let path = Path::new("/vault/pwf/PWF-0124.md");

        let error = parse_task_metadata_if_task(path, "---\nstatus: active\n---\n").unwrap_err();

        assert_matches!(
            error,
            ObsidianStoreError::MissingTaskId { path: ref actual }
                if actual == path
        );
    }

    #[test]
    fn parses_project_index_identity_into_domain_types() {
        let path = Path::new("/vault/sample-project/index.md");
        let markdown = "---\nid: smp\ntitle: sample-project\n---\n\n# Tasks\n";

        let identity = parse_project_index_identity(path, markdown).unwrap();

        assert_eq!(identity.id().as_ref(), "SMP");
        assert_eq!(project_index_frontmatter_id(&identity), "smp");
        assert_eq!(identity.title().as_ref(), "sample-project");
    }

    #[test]
    fn rejects_project_index_identity_that_disagrees_with_supplied_identity() {
        let path = Path::new("/vault/sample-project/index.md");
        let actual = ProjectIndexIdentity::new(
            ProjectId::try_new("pwf").unwrap(),
            ProjectName::try_new("pwf").unwrap(),
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
