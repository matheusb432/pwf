use std::collections::BTreeMap;

use pwf_models::project::{Project, ProjectId};
use pwf_wire::project::ProjectStatusFilter;

const PROJECTS_MAX: usize = 1024;
const BYTES_MAX: usize = 1024 * 1024;

#[derive(Default)]
pub(super) struct ProjectCache {
    projects: BTreeMap<ProjectId, Project>,
    active: Option<Vec<ProjectId>>,
    all: Option<Vec<ProjectId>>,
    bytes: usize,
}

impl ProjectCache {
    pub(super) fn get(&self, id: &ProjectId) -> Option<Project> {
        self.projects.get(id).cloned()
    }

    pub(super) fn list(&self, status: ProjectStatusFilter) -> Option<Vec<Project>> {
        let ids = if status.includes_paused() {
            self.all.as_ref()
        } else {
            self.active.as_ref()
        }?;
        ids.iter().map(|id| self.get(id)).collect()
    }

    pub(super) fn remember(&mut self, project: Project) {
        if self.projects.contains_key(&project.id) {
            return;
        }
        let bytes = project_bytes(&project);
        if self.projects.len() == PROJECTS_MAX || self.bytes.saturating_add(bytes) > BYTES_MAX {
            self.clear();
        }
        if bytes <= BYTES_MAX {
            self.bytes += bytes;
            self.projects.insert(project.id.clone(), project);
        }
    }

    pub(super) fn remember_list(&mut self, status: ProjectStatusFilter, projects: &[Project]) {
        // A full ordered result replaces partial entries and shares their project values.
        self.clear();
        let bytes = projects
            .iter()
            .map(|project| {
                project_bytes(project) + 2 * (size_of::<ProjectId>() + project.id.as_ref().len())
            })
            .sum::<usize>();
        if projects.len() > PROJECTS_MAX || bytes > BYTES_MAX {
            return;
        }
        self.bytes = bytes;
        self.projects = projects
            .iter()
            .map(|project| (project.id.clone(), project.clone()))
            .collect();
        self.active = Some(
            projects
                .iter()
                .filter(|project| !project.is_paused)
                .map(|project| project.id.clone())
                .collect(),
        );
        if status.includes_paused() {
            self.all = Some(projects.iter().map(|project| project.id.clone()).collect());
        }
    }

    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }
}

fn project_bytes(project: &Project) -> usize {
    size_of::<Project>()
        + size_of::<ProjectId>()
        + 64
        + 2 * project.id.as_ref().len()
        + project.title.as_ref().len()
        + project.tasks.path().as_ref().len()
        + project
            .source
            .as_ref()
            .map_or(0, |source| source.value().as_ref().len())
        + project
            .obsidian_vault
            .as_ref()
            .map_or(0, |vault| vault.as_ref().len())
        + project.created_at.as_ref().len()
}

#[cfg(test)]
mod tests {
    use pwf_models::project::{ProjectName, ProjectTasks, ProjectTasksKind, ProjectTasksPath};

    use super::*;

    fn project(number: usize, path: String) -> Project {
        Project {
            id: format!(
                "P{}{}{}",
                char::from(b'A' + u8::try_from(number / 676 % 26).unwrap()),
                char::from(b'A' + u8::try_from(number / 26 % 26).unwrap()),
                char::from(b'A' + u8::try_from(number % 26).unwrap())
            )
            .parse()
            .unwrap(),
            title: ProjectName::try_new(format!("Project {number}")).unwrap(),
            source: None,
            tasks: ProjectTasks::new(
                ProjectTasksKind::Directory,
                ProjectTasksPath::try_new(path).unwrap(),
            ),
            obsidian_vault: None,
            snapshot_enabled: false,
            is_paused: false,
            created_at: "2026-09-13T00:00:00Z".parse().unwrap(),
        }
    }

    #[test]
    fn capacity_eviction_discards_ordered_views_and_oversized_results_bypass_retention() {
        let mut cache = ProjectCache::default();
        let first = project(0, "/tasks/first".into());
        cache.remember_list(ProjectStatusFilter::IncludingPaused, &[first]);
        for number in 1..=PROJECTS_MAX {
            cache.remember(project(number, format!("/tasks/{number}")));
        }
        assert!(cache.projects.len() <= PROJECTS_MAX);
        assert!(cache.bytes <= BYTES_MAX);
        assert!(cache.list(ProjectStatusFilter::IncludingPaused).is_none());
        let oversized = project(0, "x".repeat(BYTES_MAX));
        cache.remember(oversized.clone());
        assert!(cache.projects.is_empty());
        cache.remember_list(ProjectStatusFilter::IncludingPaused, &[oversized]);
        assert!(cache.projects.is_empty());
        assert!(cache.list(ProjectStatusFilter::IncludingPaused).is_none());
    }
}
