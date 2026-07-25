use std::fmt;

use nutype::nutype;
use thiserror::Error;

/// Names a managed project.
#[nutype(
    sanitize(trim),
    validate(predicate = is_project_name),
    derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, AsRef, Display,)
)]
pub struct ProjectName(String);

/// Identifies a managed project in pending-work identifiers.
#[nutype(
    sanitize(trim, uppercase),
    validate(predicate = is_project_prefix),
    derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, AsRef, Display,)
)]
pub struct ProjectPrefix(String);

/// Identifies one configured project's index independently of its filename.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectIndexIdentity {
    id: ProjectPrefix,
    title: ProjectName,
}

impl ProjectIndexIdentity {
    /// Creates a project index identity.
    pub fn new(id: ProjectPrefix, title: ProjectName) -> Self {
        Self { id, title }
    }

    /// Returns the canonical uppercase project prefix.
    pub fn id(&self) -> &ProjectPrefix {
        &self.id
    }

    /// Returns the lowercase prefix stored in project-index frontmatter.
    pub fn frontmatter_id(&self) -> String {
        self.id.as_ref().to_ascii_lowercase()
    }

    /// Returns the configured project name stored as the index title.
    pub fn title(&self) -> &ProjectName {
        &self.title
    }
}

/// Identifies the supported project source location kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectSourceKind {
    /// A local directory.
    Directory,
}

impl fmt::Display for ProjectSourceKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("directory")
    }
}

impl TryFrom<&str> for ProjectSourceKind {
    type Error = ProjectSourceKindError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "directory" => Ok(Self::Directory),
            _ => Err(ProjectSourceKindError),
        }
    }
}

/// Reports an unsupported project source location kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("project source kind must be directory")]
pub struct ProjectSourceKindError;

/// Stores a non-empty project source location.
#[nutype(
    validate(predicate = is_not_blank),
    derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, AsRef, Display,)
)]
pub struct ProjectSourceValue(String);

/// Locates a managed project's source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectSource {
    kind: ProjectSourceKind,
    value: ProjectSourceValue,
}

impl ProjectSource {
    /// Creates a project source location.
    pub fn new(kind: ProjectSourceKind, value: ProjectSourceValue) -> Self {
        Self { kind, value }
    }

    /// Returns the source location kind.
    pub fn kind(&self) -> ProjectSourceKind {
        self.kind
    }

    /// Returns the source location value.
    pub fn value(&self) -> &ProjectSourceValue {
        &self.value
    }
}

/// Identifies the supported project tasks location kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectTasksKind {
    /// A local directory.
    Directory,
}

impl fmt::Display for ProjectTasksKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("directory")
    }
}

impl TryFrom<&str> for ProjectTasksKind {
    type Error = ProjectTasksKindError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "directory" => Ok(Self::Directory),
            _ => Err(ProjectTasksKindError),
        }
    }
}

/// Reports an unsupported project tasks location kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("project tasks kind must be directory")]
pub struct ProjectTasksKindError;

/// Stores a non-empty project tasks path.
#[nutype(
    validate(predicate = is_not_blank),
    derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, AsRef, Display,)
)]
pub struct ProjectTasksPath(String);

/// Locates a managed project's pending-work files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectTasks {
    kind: ProjectTasksKind,
    path: ProjectTasksPath,
}

impl ProjectTasks {
    /// Creates a project tasks location.
    pub fn new(kind: ProjectTasksKind, path: ProjectTasksPath) -> Self {
        Self { kind, path }
    }

    /// Returns the tasks location kind.
    pub fn kind(&self) -> ProjectTasksKind {
        self.kind
    }

    /// Returns the tasks location path.
    pub fn path(&self) -> &ProjectTasksPath {
        &self.path
    }
}

fn is_project_name(raw: &str) -> bool {
    !raw.is_empty() && !raw.eq_ignore_ascii_case("project")
}

fn is_project_prefix(raw: &str) -> bool {
    (2..=4).contains(&raw.len()) && raw.chars().all(|ch| ch.is_ascii_uppercase())
}

fn is_not_blank(raw: &str) -> bool {
    !raw.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_prefix_normalizes_two_to_four_ascii_letters() {
        assert_eq!(ProjectPrefix::try_new(" pwf ").unwrap().as_ref(), "PWF");
        assert!(ProjectPrefix::try_new("P").is_err());
        assert!(ProjectPrefix::try_new("TOOLS").is_err());
    }

    #[test]
    fn project_index_identity_uses_typed_config_identity() {
        let identity = ProjectIndexIdentity::new(
            ProjectPrefix::try_new("pwf").unwrap(),
            ProjectName::try_new("pwf").unwrap(),
        );

        assert_eq!(identity.id().as_ref(), "PWF");
        assert_eq!(identity.frontmatter_id(), "pwf");
        assert_eq!(identity.title().as_ref(), "pwf");
    }

    #[test]
    fn project_name_rejects_the_reserved_command_name() {
        for value in ["project", " Project ", "PROJECT"] {
            assert!(ProjectName::try_new(value).is_err());
        }
        assert_eq!(ProjectName::try_new("pwf").unwrap().as_ref(), "pwf");
    }

    #[test]
    fn project_name_trims_and_rejects_blank() {
        assert_eq!(
            ProjectName::try_new("  foo-bar  ").unwrap().as_ref(),
            "foo-bar"
        );
        assert!(ProjectName::try_new(" \t ").is_err());
    }

    #[test]
    fn source_and_task_locations_reject_blank_values() {
        assert!(ProjectSourceValue::try_new(" \t ").is_err());
        assert!(ProjectTasksPath::try_new(" \t ").is_err());
    }

    #[test]
    fn project_source_value_retains_accepted_whitespace() {
        let value = " /work/pwf ";

        assert_eq!(ProjectSourceValue::try_new(value).unwrap().as_ref(), value);
    }

    #[test]
    fn project_tasks_path_retains_accepted_whitespace() {
        let path = " .pending-work ";

        assert_eq!(ProjectTasksPath::try_new(path).unwrap().as_ref(), path);
    }

    #[test]
    fn kinds_accept_only_directory() {
        assert_eq!(
            ProjectSourceKind::try_from("directory").unwrap(),
            ProjectSourceKind::Directory
        );
        assert_eq!(
            ProjectTasksKind::try_from("directory").unwrap(),
            ProjectTasksKind::Directory
        );
        assert!(ProjectSourceKind::try_from("remote").is_err());
        assert!(ProjectTasksKind::try_from("remote").is_err());
    }
}
