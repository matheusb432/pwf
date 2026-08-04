use std::{fmt, str::FromStr};

use nutype::nutype;
use thiserror::Error;

/// Names a managed project.
#[nutype(
    sanitize(trim),
    validate(predicate = is_project_name),
    derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, AsRef, Display,)
)]
pub struct ProjectName(String);

/// Identifies a managed project in task identifiers.
#[nutype(
    sanitize(trim, uppercase),
    validate(predicate = is_project_id),
    derive(
        Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, AsRef, Display, FromStr,
    )
)]
pub struct ProjectId(String);

/// Selects a managed project by its configured name or ID.
///
/// The original spelling is retained for name lookup and diagnostics. When the
/// value is also a valid project ID, resolution can fall back to that ID after
/// checking for an exact name match.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProjectSelector {
    value: String,
    project_id: Option<ProjectId>,
}

impl ProjectSelector {
    /// Returns the project ID candidate, when the selector has ID syntax.
    #[must_use]
    pub fn project_id(&self) -> Option<&ProjectId> {
        self.project_id.as_ref()
    }
}

impl AsRef<str> for ProjectSelector {
    fn as_ref(&self) -> &str {
        &self.value
    }
}

impl fmt::Display for ProjectSelector {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.value)
    }
}

impl FromStr for ProjectSelector {
    type Err = ProjectSelectorError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let value = raw.trim();
        if value.is_empty() {
            return Err(ProjectSelectorError);
        }
        Ok(Self {
            value: value.to_string(),
            project_id: ProjectId::try_new(value).ok(),
        })
    }
}

/// Reports an empty managed-project selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("project selector cannot be blank")]
pub struct ProjectSelectorError;

/// Identifies one configured project's index independently of its filename.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectIndexIdentity {
    id: ProjectId,
    title: ProjectName,
}

impl ProjectIndexIdentity {
    /// Creates a project index identity.
    pub fn new(id: ProjectId, title: ProjectName) -> Self {
        Self { id, title }
    }

    /// Returns the uppercase project ID.
    pub fn id(&self) -> &ProjectId {
        &self.id
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

/// Locates a managed project's task files.
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

/// Describes one managed project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// Project ID.
    pub id: ProjectId,
    /// Project title.
    pub title: ProjectName,
    /// Project source location.
    pub source: ProjectSource,
    /// Task location.
    pub tasks: ProjectTasks,
    /// RFC 3339 UTC creation timestamp.
    pub created_at: String,
    /// Reports whether the project is paused.
    pub is_paused: bool,
}

fn is_project_name(raw: &str) -> bool {
    !raw.is_empty() && !raw.eq_ignore_ascii_case("project")
}

fn is_project_id(raw: &str) -> bool {
    raw.len() == 3 && raw.chars().all(|ch| ch.is_ascii_uppercase())
}

fn is_not_blank(raw: &str) -> bool {
    !raw.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_id_parses_exactly_three_ascii_letters() {
        assert_eq!(" pwf ".parse::<ProjectId>().unwrap().as_ref(), "PWF");
        assert!("PW".parse::<ProjectId>().is_err());
        assert!("TOOL".parse::<ProjectId>().is_err());
    }

    #[test]
    fn project_selector_retains_names_and_exposes_id_candidates() {
        let name = " config-handler ".parse::<ProjectSelector>().unwrap();
        assert_eq!(name.as_ref(), "config-handler");
        assert_eq!(name.project_id(), None);

        let id = " pwf ".parse::<ProjectSelector>().unwrap();
        assert_eq!(id.as_ref(), "pwf");
        assert_eq!(id.project_id().map(AsRef::as_ref), Some("PWF"));
        assert!(" \t ".parse::<ProjectSelector>().is_err());
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
