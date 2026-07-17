use std::collections::BTreeMap;

use super::{ProjectName, WorkItemId};

/// Associates a managed project with its repository and uppercase item-id prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ProjectEntry {
    repo: Option<String>,
    prefix: Option<String>,
}

/// Routes work-item ids and repository lookups through managed project configuration.
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
                .map(|(name, repo, prefix)| (name, ProjectEntry { repo, prefix }))
                .collect(),
        }
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
            .map(|(name, entry)| (name, entry.repo.as_deref()))
    }

    /// The configured repo for `project`, if the project is managed and mapped.
    pub fn repo_for(&self, project: &ProjectName) -> Option<&str> {
        self.entries
            .get(project)
            .and_then(|entry| entry.repo.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> ProjectRegistry {
        ProjectRegistry::new(vec![(
            ProjectName::try_new("pwf").unwrap(),
            Some("/repo/pwf".to_string()),
            Some("PWF".to_string()),
        )])
    }

    #[test]
    fn registry_resolves_project_by_id_prefix() {
        let registry = registry();
        let id = WorkItemId::try_new("PWF-0123").unwrap();
        let resolved = registry.project_for_id(&id);

        assert!(resolved.is_some());
        assert_eq!(resolved.unwrap().as_ref(), "pwf");
    }

    #[test]
    fn registry_misses_unknown_prefix() {
        let registry = registry();
        let id = WorkItemId::try_new("XYZ-0123").unwrap();
        assert!(registry.project_for_id(&id).is_none());
    }

    #[test]
    fn registry_enumerates_projects_with_repo() {
        let got: Vec<_> = registry()
            .projects()
            .map(|(name, repo)| (name.as_ref().to_string(), repo.map(str::to_string)))
            .collect();
        assert_eq!(got, [("pwf".to_string(), Some("/repo/pwf".to_string()))]);
    }

    #[test]
    fn registry_returns_repo_for_project() {
        let pwf = ProjectName::try_new("pwf").unwrap();
        assert_eq!(registry().repo_for(&pwf), Some("/repo/pwf"));
    }
}
