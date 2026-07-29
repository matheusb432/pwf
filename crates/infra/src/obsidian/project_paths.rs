use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use pwf_models::pending_work::{ProjectIndexIdentity, ProjectName};

use super::ObsidianStoreError;

#[derive(Clone)]
pub struct ObsidianProject {
    identity: ProjectIndexIdentity,
    tasks_path: PathBuf,
}

impl ObsidianProject {
    pub fn new(identity: ProjectIndexIdentity, tasks_path: PathBuf) -> Self {
        Self {
            identity,
            tasks_path,
        }
    }
}

#[derive(Clone)]
pub(super) struct ProjectPaths {
    projects: BTreeMap<ProjectName, ProjectPathEntry>,
}

#[derive(Clone)]
struct ProjectPathEntry {
    identity: ProjectIndexIdentity,
    tasks_path: PathBuf,
}

impl ProjectPaths {
    pub(super) fn from_projects(projects: impl IntoIterator<Item = ObsidianProject>) -> Self {
        Self {
            projects: projects
                .into_iter()
                .map(|project| {
                    (
                        project.identity.title().clone(),
                        ProjectPathEntry {
                            identity: project.identity,
                            tasks_path: project.tasks_path,
                        },
                    )
                })
                .collect(),
        }
    }

    pub(super) fn project_directory(
        &self,
        project: &ProjectName,
    ) -> Result<&Path, ObsidianStoreError> {
        Ok(&self.project(project)?.tasks_path)
    }

    pub(super) fn project_index_path(
        &self,
        project: &ProjectName,
    ) -> Result<PathBuf, ObsidianStoreError> {
        let tasks_path = &self.project(project)?.tasks_path;
        Ok(tasks_path.join(format!("{project}.md")))
    }

    pub(super) fn project_identity(
        &self,
        project: &ProjectName,
    ) -> Result<&ProjectIndexIdentity, ObsidianStoreError> {
        Ok(&self.project(project)?.identity)
    }

    fn project(&self, project: &ProjectName) -> Result<&ProjectPathEntry, ObsidianStoreError> {
        self.projects
            .get(project)
            .ok_or_else(|| ObsidianStoreError::UnknownProject {
                project: project.as_ref().to_string(),
            })
    }
}
