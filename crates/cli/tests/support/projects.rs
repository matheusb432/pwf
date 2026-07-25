use std::path::PathBuf;

use pwf_application::pending_work::ProjectRegistry;
use pwf_domain::pending_work::{ProjectIndexIdentity, ProjectName, ProjectPrefix};
use pwf_infra::obsidian::{ObsidianProject, ObsidianStore};

pub struct TestProject {
    pub id: &'static str,
    pub title: &'static str,
    pub repository: PathBuf,
    pub tasks_path: PathBuf,
}

pub struct TestProjects {
    pub registry: ProjectRegistry,
    pub store: ObsidianStore,
}

impl TestProjects {
    pub fn new(projects: impl IntoIterator<Item = TestProject>) -> Self {
        let projects = projects
            .into_iter()
            .map(|project| {
                let id = ProjectPrefix::try_new(project.id).expect("valid test project id");
                let title = ProjectName::try_new(project.title).expect("valid test project title");
                (id, title, project.repository, project.tasks_path)
            })
            .collect::<Vec<_>>();
        let registry = ProjectRegistry::new(projects.iter().map(|(id, title, repository, _)| {
            (
                title.clone(),
                Some(repository.to_string_lossy().into_owned()),
                Some(id.to_string()),
            )
        }));
        let store = ObsidianStore::new(projects.into_iter().map(|(id, title, _, tasks_path)| {
            ObsidianProject::new(ProjectIndexIdentity::new(id, title), tasks_path)
        }));
        Self { registry, store }
    }
}
