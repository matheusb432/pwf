use std::{collections::BTreeMap, path::Path};

use pwf_domain::{
    pending_work::{ProjectName, WorkItemId},
    project::ProjectPrefix,
};

use crate::project::Project;

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
/// use pwf_application::pending_work::ProjectRegistry;
/// use pwf_domain::pending_work::ProjectName;
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
    use pwf_domain::project::{
        ProjectName, ProjectPrefix, ProjectSource, ProjectSourceKind, ProjectSourceValue,
        ProjectTasks, ProjectTasksKind, ProjectTasksPath,
    };

    use super::ProjectRegistry;
    use crate::project::Project;

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
