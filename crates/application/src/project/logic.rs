use std::{collections::BTreeMap, path::Path};

use pwf_models::{
    pending_work::{ProjectName, WorkItemId},
    project::{Project, ProjectPrefix},
};

use super::dto::{ProjectRow, ProjectRowError};

pub(super) fn project_from_row(row: ProjectRow) -> Result<Project, ProjectRowError> {
    let id = project_value("id", row.id, ProjectPrefix::try_new)?;
    let title = project_value("title", row.title, ProjectName::try_new)?;
    let source_kind = project_value("source kind", row.source_kind, |value| {
        pwf_models::project::ProjectSourceKind::try_from(value.as_str())
    })?;
    let source_value = project_value(
        "source value",
        row.source_value,
        pwf_models::project::ProjectSourceValue::try_new,
    )?;
    let tasks_kind = project_value("tasks kind", row.tasks_kind, |value| {
        pwf_models::project::ProjectTasksKind::try_from(value.as_str())
    })?;
    let tasks_path = project_value(
        "tasks path",
        row.tasks_path,
        pwf_models::project::ProjectTasksPath::try_new,
    )?;
    if row.created_at.is_empty() {
        return Err(ProjectRowError {
            field: "created_at",
            value: row.created_at,
            reason: "value cannot be empty".to_string(),
        });
    }

    Ok(Project {
        id,
        title,
        source: pwf_models::project::ProjectSource::new(source_kind, source_value),
        tasks: pwf_models::project::ProjectTasks::new(tasks_kind, tasks_path),
        created_at: row.created_at,
        is_paused: row.is_paused,
    })
}

fn project_value<T, E>(
    field: &'static str,
    value: String,
    conversion: impl FnOnce(String) -> Result<T, E>,
) -> Result<T, ProjectRowError>
where
    E: std::fmt::Display,
{
    conversion(value.clone()).map_err(|error| ProjectRowError {
        field,
        value,
        reason: error.to_string(),
    })
}

/// Associates a managed project with its repository and item-id prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ProjectEntry {
    repository: Option<String>,
    prefix: Option<String>,
}

/// Reports that no project or several projects matched a managed-project lookup.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ProjectResolutionError {
    /// No configured project matched the supplied identifier.
    #[error(
        "Unknown managed project identifier: {identifier}\nManaged project identifiers: {}",
        known.join(", ")
    )]
    Unknown {
        /// Identifier supplied by the caller.
        identifier: String,
        /// Configured project names in deterministic order.
        known: Vec<String>,
    },
    /// Several configured projects matched the first non-empty lookup tier.
    #[error(
        "'{identifier}' is ambiguous. Managed project identifiers matching it: {}.",
        matches.join(", ")
    )]
    Ambiguous {
        /// Identifier supplied by the caller.
        identifier: String,
        /// Project names matched by the first non-empty lookup tier.
        matches: Vec<String>,
    },
}

/// Routes managed-project identifiers, work-item ids, and repository paths.
///
/// # Examples
///
/// ```
/// use pwf_application::project::ProjectRegistry;
/// use pwf_models::pending_work::ProjectName;
///
/// let registry = ProjectRegistry::new([(
///     ProjectName::try_new("pwf").unwrap(),
///     Some("/repo/pwf".to_string()),
///     Some("PWF".to_string()),
/// )]);
///
/// assert_eq!(registry.resolve("PWF").unwrap().as_ref(), "pwf");
/// ```
#[derive(Debug, Clone, Default)]
pub struct ProjectRegistry {
    entries: BTreeMap<ProjectName, ProjectEntry>,
}

impl ProjectRegistry {
    /// Builds a registry from `(project, repository, uppercase prefix)` triples.
    pub fn new(
        entries: impl IntoIterator<Item = (ProjectName, Option<String>, Option<String>)>,
    ) -> Self {
        Self {
            entries: entries
                .into_iter()
                .map(|(name, repository, prefix)| (name, ProjectEntry { repository, prefix }))
                .collect(),
        }
    }

    /// Builds routing entries from persisted projects after expanding directory sources.
    pub fn from_projects(projects: &[Project], expand_directory: impl Fn(&str) -> String) -> Self {
        Self::new(projects.iter().map(|project| {
            (
                project.title.clone(),
                Some(expand_directory(project.source.value().as_ref())),
                Some(project.id.to_string()),
            )
        }))
    }

    /// Resolves an exact name, a unique ASCII-case-insensitive name, or a unique prefix.
    ///
    /// Name matching stops before prefix matching when the name tier is non-empty.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectResolutionError::Unknown`] when no tier matches, or
    /// [`ProjectResolutionError::Ambiguous`] when the first non-empty tier has several matches.
    pub fn resolve(&self, identifier: &str) -> Result<&ProjectName, ProjectResolutionError> {
        if let Some((name, _)) = self
            .entries
            .iter()
            .find(|(name, _)| name.as_ref() == identifier)
        {
            return Ok(name);
        }

        let names = self
            .entries
            .keys()
            .filter(|name| name.as_ref().eq_ignore_ascii_case(identifier))
            .collect();
        if let Some(result) = resolve_tier(identifier, names) {
            return result;
        }

        let prefixes = self
            .entries
            .iter()
            .filter(|(_, entry)| {
                entry
                    .prefix
                    .as_deref()
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case(identifier))
            })
            .map(|(name, _)| name)
            .collect();
        if let Some(result) = resolve_tier(identifier, prefixes) {
            return result;
        }

        Err(ProjectResolutionError::Unknown {
            identifier: identifier.to_string(),
            known: self.project_names(),
        })
    }

    /// Resolves one project after slash-direction, trailing-separator, and ASCII-case
    /// normalization.
    ///
    /// # Errors
    ///
    /// Returns [`ProjectResolutionError::Unknown`] when no configured repository matches.
    /// Duplicate normalized mappings preserve project-name ordering and select the first match.
    pub fn project_for_repository(
        &self,
        repository: impl AsRef<Path>,
    ) -> Result<&ProjectName, ProjectResolutionError> {
        let repository = repository.as_ref().to_string_lossy();
        let normalized = normalize_repository(&repository);
        self.entries
            .iter()
            .filter(|(_, entry)| {
                entry
                    .repository
                    .as_deref()
                    .is_some_and(|repository| normalize_repository(repository) == normalized)
            })
            .map(|(name, _)| name)
            .next()
            .ok_or_else(|| ProjectResolutionError::Unknown {
                identifier: repository.into_owned(),
                known: self.project_names(),
            })
    }

    /// Resolves the project owning `id` by its case-sensitive uppercase prefix.
    pub fn project_for_id(&self, id: &WorkItemId) -> Option<&ProjectName> {
        let prefix = id.as_ref().split('-').next()?;
        self.entries
            .iter()
            .find(|(_, entry)| entry.prefix.as_deref() == Some(prefix))
            .map(|(name, _)| name)
    }

    /// Counts projects using `prefix`; a count above one makes item routing ambiguous.
    pub fn projects_with_prefix(&self, prefix: &str) -> usize {
        self.entries
            .values()
            .filter(|entry| entry.prefix.as_deref() == Some(prefix))
            .count()
    }

    /// Returns managed projects and repositories in project-name order.
    pub fn projects(&self) -> impl Iterator<Item = (&ProjectName, Option<&str>)> {
        self.entries
            .iter()
            .map(|(name, entry)| (name, entry.repository.as_deref()))
    }

    /// Returns the configured repository for `project`, when present.
    pub fn repo_for(&self, project: &ProjectName) -> Option<&str> {
        self.entries
            .get(project)
            .and_then(|entry| entry.repository.as_deref())
    }

    /// Returns the managed-project prefix for `project`, when present.
    pub fn prefix_for(&self, project: &ProjectName) -> Option<ProjectPrefix> {
        self.entries
            .get(project)
            .and_then(|entry| entry.prefix.as_deref())
            .and_then(|prefix| ProjectPrefix::try_new(prefix).ok())
    }

    fn project_names(&self) -> Vec<String> {
        self.entries.keys().map(ToString::to_string).collect()
    }
}

fn resolve_tier<'registry>(
    identifier: &str,
    matches: Vec<&'registry ProjectName>,
) -> Option<Result<&'registry ProjectName, ProjectResolutionError>> {
    match matches.as_slice() {
        [] => None,
        [project] => Some(Ok(*project)),
        _ => Some(Err(ProjectResolutionError::Ambiguous {
            identifier: identifier.to_string(),
            matches: matches.into_iter().map(ToString::to_string).collect(),
        })),
    }
}

fn normalize_repository(repository: &str) -> String {
    repository
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use pwf_models::project::{
        Project, ProjectName, ProjectPrefix, ProjectSource, ProjectSourceKind, ProjectSourceValue,
        ProjectTasks, ProjectTasksKind, ProjectTasksPath,
    };

    use super::{ProjectRegistry, ProjectResolutionError};

    fn project(name: &str) -> ProjectName {
        ProjectName::try_new(name).unwrap()
    }

    fn registry() -> ProjectRegistry {
        ProjectRegistry::new([
            (
                project("pwf"),
                Some("/repo/pwf".to_string()),
                Some("PWF".to_string()),
            ),
            (
                project("alpha"),
                Some(r"C:\repo\alpha\".to_string()),
                Some("DUP".to_string()),
            ),
            (
                project("beta"),
                Some("/repo/beta".to_string()),
                Some("DUP".to_string()),
            ),
        ])
    }

    #[test]
    fn resolves_name_before_prefix_and_reports_structured_failures() {
        let registry = registry();

        assert_eq!(registry.resolve("pwf").unwrap().as_ref(), "pwf");
        assert_eq!(registry.resolve("PWF").unwrap().as_ref(), "pwf");
        assert_matches!(
            registry.resolve("pw"),
            Err(ProjectResolutionError::Unknown {
                ref identifier,
                ref known,
            }) if identifier == "pw"
                && known == &vec!["alpha".to_string(), "beta".to_string(), "pwf".to_string()]
        );
        assert_matches!(
            registry.resolve("dup"),
            Err(ProjectResolutionError::Ambiguous {
                ref identifier,
                ref matches,
            }) if identifier == "dup"
                && matches == &vec!["alpha".to_string(), "beta".to_string()]
        );
    }

    #[test]
    fn stops_at_the_first_non_empty_resolution_tier() {
        let registry = ProjectRegistry::new([
            (
                project("PWF"),
                Some("/repo/upper".to_string()),
                Some("UPR".to_string()),
            ),
            (
                project("pwf"),
                Some("/repo/lower".to_string()),
                Some("PWF".to_string()),
            ),
        ]);

        assert_eq!(registry.resolve("pwf").unwrap().as_ref(), "pwf");
        assert_matches!(
            registry.resolve("PwF"),
            Err(ProjectResolutionError::Ambiguous { ref matches, .. })
                if matches == &vec!["PWF".to_string(), "pwf".to_string()]
        );
    }

    #[test]
    fn matches_repositories_after_only_contract_normalization() {
        let registry = registry();

        assert_eq!(
            registry
                .project_for_repository("/REPO/PWF/")
                .unwrap()
                .as_ref(),
            "pwf"
        );
        assert_eq!(
            registry
                .project_for_repository("c:/REPO/alpha")
                .unwrap()
                .as_ref(),
            "alpha"
        );
        assert_matches!(
            registry.project_for_repository("/repo/./pwf"),
            Err(ProjectResolutionError::Unknown { .. })
        );
    }

    #[test]
    fn duplicate_repository_mappings_select_the_first_project() {
        let registry = ProjectRegistry::new([
            (
                project("alpha"),
                Some("/repo/shared".to_string()),
                Some("ALP".to_string()),
            ),
            (
                project("beta"),
                Some(r"\REPO\SHARED\".to_string()),
                Some("BET".to_string()),
            ),
        ]);

        assert_eq!(
            registry
                .project_for_repository("/repo/shared/")
                .unwrap()
                .as_ref(),
            "alpha"
        );
    }

    #[test]
    fn project_rows_build_repository_and_prefix_routes_with_expanded_sources() {
        let projects = [Project {
            id: ProjectPrefix::try_new("pwf").unwrap(),
            title: ProjectName::try_new("pwf").unwrap(),
            source: ProjectSource::new(
                ProjectSourceKind::Directory,
                ProjectSourceValue::try_new("~/tools/pwf").unwrap(),
            ),
            tasks: ProjectTasks::new(
                ProjectTasksKind::Directory,
                ProjectTasksPath::try_new("~/notes/pwf").unwrap(),
            ),
            created_at: "2026-07-25T00:00:00Z".to_string(),
            is_paused: false,
        }];

        let registry =
            ProjectRegistry::from_projects(&projects, |path| path.replacen('~', "/home/me", 1));

        let project = registry.resolve("PWF").unwrap();
        assert_eq!(project.as_ref(), "pwf");
        assert_eq!(registry.repo_for(project), Some("/home/me/tools/pwf"));
    }
}

pub(in crate::project) mod task_location {
    use std::path::{Path, PathBuf};

    use pwf_models::project::ProjectPrefix;

    use crate::project::resolve_runtime_path::{ResolvedPath, RuntimePathError, resolve};

    #[derive(Debug)]
    pub(in crate::project) enum TaskLocationError {
        InvalidPath {
            project_id: ProjectPrefix,
            path: String,
            source: RuntimePathError,
        },
        Collision {
            first_id: ProjectPrefix,
            second_id: ProjectPrefix,
            path: PathBuf,
        },
    }

    pub(in crate::project) fn reject_collision(
        candidate_id: &ProjectPrefix,
        candidate_path: &str,
        existing: impl IntoIterator<Item = (ProjectPrefix, String)>,
        home: &Path,
    ) -> Result<ResolvedPath, TaskLocationError> {
        let candidate = resolve_path(candidate_id, candidate_path, home)?;
        let mut existing = existing.into_iter().collect::<Vec<_>>();
        existing.sort_unstable_by(|(left_id, _), (right_id, _)| left_id.cmp(right_id));

        for (existing_id, existing_path) in existing {
            let existing = resolve_path(&existing_id, &existing_path, home)?;
            if candidate.identity() == existing.identity() {
                let (first_id, second_id) = if candidate_id <= &existing_id {
                    (candidate_id.clone(), existing_id)
                } else {
                    (existing_id, candidate_id.clone())
                };
                return Err(TaskLocationError::Collision {
                    first_id,
                    second_id,
                    path: candidate.path().to_path_buf(),
                });
            }
        }

        Ok(candidate)
    }

    fn resolve_path(
        project_id: &ProjectPrefix,
        path: &str,
        home: &Path,
    ) -> Result<ResolvedPath, TaskLocationError> {
        resolve(path, home).map_err(|source| TaskLocationError::InvalidPath {
            project_id: project_id.clone(),
            path: path.to_string(),
            source,
        })
    }
}
