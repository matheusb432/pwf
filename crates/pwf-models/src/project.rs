use std::{
    fmt,
    path::{Path, PathBuf},
    str::FromStr,
};

use nutype::nutype;
use thiserror::Error;

/// Maximum Unicode scalar count accepted for one managed-project name.
pub const PROJECT_NAME_CHARACTER_LIMIT: usize = 200;

/// Identifies the host home directory used to resolve managed-project paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeDirectory(PathBuf);

impl HomeDirectory {
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self(path)
    }

    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

/// Reports why a managed-project name is invalid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ProjectNameError {
    #[error("project name cannot be blank")]
    Blank,
    #[error("project name 'project' is reserved")]
    Reserved,
    #[error("project name cannot exceed {PROJECT_NAME_CHARACTER_LIMIT} characters")]
    TooLong,
    #[error("project name cannot contain path separators or control characters")]
    UnsafeCharacter,
}

/// Names a managed project with a bounded value safe to use as one path component.
#[nutype(
    sanitize(trim),
    validate(with = validate_project_name, error = ProjectNameError),
    derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, AsRef, Display,)
)]
pub struct ProjectName(String);

/// Reports a project-creation value that is not a UTC timestamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("project creation timestamp must be an RFC 3339 UTC value")]
pub struct ProjectCreatedAtError;

/// Stores one persisted UTC project-creation timestamp.
#[nutype(
    validate(with = validate_utc_timestamp, error = ProjectCreatedAtError),
    derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, AsRef, Display, FromStr,)
)]
pub struct ProjectCreatedAt(String);

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
    #[must_use]
    pub fn new(id: ProjectId, title: ProjectName) -> Self {
        Self { id, title }
    }

    /// Returns the uppercase project ID.
    #[must_use]
    pub fn id(&self) -> &ProjectId {
        &self.id
    }

    /// Returns the configured project name stored as the index title.
    #[must_use]
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
    #[must_use]
    pub fn new(kind: ProjectSourceKind, value: ProjectSourceValue) -> Self {
        Self { kind, value }
    }

    /// Returns the source location kind.
    #[must_use]
    pub fn kind(&self) -> ProjectSourceKind {
        self.kind
    }

    /// Returns the source location value.
    #[must_use]
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
    #[must_use]
    pub fn new(kind: ProjectTasksKind, path: ProjectTasksPath) -> Self {
        Self { kind, path }
    }

    /// Returns the tasks location kind.
    #[must_use]
    pub fn kind(&self) -> ProjectTasksKind {
        self.kind
    }

    /// Returns the tasks location path.
    #[must_use]
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
    pub created_at: ProjectCreatedAt,
    /// Reports whether the project is paused.
    pub is_paused: bool,
}

fn validate_project_name(raw: &str) -> Result<(), ProjectNameError> {
    if raw.is_empty() {
        return Err(ProjectNameError::Blank);
    }
    if raw.eq_ignore_ascii_case("project") {
        return Err(ProjectNameError::Reserved);
    }
    if raw.chars().count() > PROJECT_NAME_CHARACTER_LIMIT {
        return Err(ProjectNameError::TooLong);
    }
    if raw
        .chars()
        .any(|character| character.is_control() || matches!(character, '/' | '\\'))
    {
        return Err(ProjectNameError::UnsafeCharacter);
    }
    Ok(())
}

fn validate_utc_timestamp(raw: &str) -> Result<(), ProjectCreatedAtError> {
    if raw.ends_with('Z') && raw.parse::<jiff::Timestamp>().is_ok() {
        Ok(())
    } else {
        Err(ProjectCreatedAtError)
    }
}

fn is_project_id(raw: &str) -> bool {
    (2..=4).contains(&raw.len()) && raw.chars().all(|ch| ch.is_ascii_uppercase())
}

fn is_not_blank(raw: &str) -> bool {
    !raw.trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_id_parses_two_to_four_ascii_letters() {
        for (raw, expected) in [(" pw ", "PW"), (" foo ", "FOO"), (" tool ", "TOOL")] {
            assert_eq!(raw.parse::<ProjectId>().unwrap().as_ref(), expected);
        }
        for raw in ["P", "TOOLS", "P1", "P_E"] {
            assert!(raw.parse::<ProjectId>().is_err(), "accepted {raw:?}");
        }
    }

    #[test]
    fn project_selector_retains_names_and_exposes_id_candidates() {
        let name = " companion-project ".parse::<ProjectSelector>().unwrap();
        assert_eq!(name.as_ref(), "companion-project");
        assert_eq!(name.project_id(), None);

        let id = " foo ".parse::<ProjectSelector>().unwrap();
        assert_eq!(id.as_ref(), "foo");
        assert_eq!(id.project_id().map(AsRef::as_ref), Some("FOO"));
        assert_eq!(
            " tool "
                .parse::<ProjectSelector>()
                .unwrap()
                .project_id()
                .map(AsRef::as_ref),
            Some("TOOL")
        );
        assert!(" \t ".parse::<ProjectSelector>().is_err());
    }

    #[test]
    fn project_name_rejects_the_reserved_command_name() {
        for value in ["project", " Project ", "PROJECT"] {
            assert!(ProjectName::try_new(value).is_err());
        }
        assert_eq!(ProjectName::try_new("foo").unwrap().as_ref(), "foo");
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
