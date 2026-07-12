use std::path::{Path, PathBuf};

use pwf_application::PendingWorkReadStore;
use pwf_domain::pending_work::{OpenItem, ProjectName};

use super::{
    ObsidianPendingWorkStore, ObsidianPendingWorkStoreError, fs::read_text_optional,
    lookup::TaskFile, read_parser::parse_project_tasks,
};
use crate::obsidian::identity::{
    configured_project_index_identity, parse_project_index_identity,
    validate_project_index_identity,
};

impl PendingWorkReadStore for ObsidianPendingWorkStore {
    type Error = ObsidianPendingWorkStoreError;

    fn open_items_for_project(&self, project: &ProjectName) -> Result<Vec<OpenItem>, Self::Error> {
        self.open_items_for_projects(std::slice::from_ref(project))
    }

    fn all_open_items(&self) -> Result<Vec<OpenItem>, Self::Error> {
        let projects = self
            .config
            .projects
            .keys()
            .map(|project| ProjectName::try_new(project).expect("configured project is non-empty"))
            .collect::<Vec<_>>();
        self.open_items_for_projects(&projects)
    }

    fn open_item(&self, id: &str) -> Result<OpenItem, Self::Error> {
        self.find_pending_item(id)
    }
}

impl ObsidianPendingWorkStore {
    fn open_items_for_projects(
        &self,
        project_names: &[ProjectName],
    ) -> Result<Vec<OpenItem>, ObsidianPendingWorkStoreError> {
        if !Path::new(&self.config.notes_dir).exists() {
            return Err(ObsidianPendingWorkStoreError::NotesDirectoryNotFound {
                path: self.config.notes_dir.clone(),
            });
        }

        let mut items = Vec::new();
        for project in project_names {
            let repo = self
                .config
                .projects
                .get(project.as_ref())
                .map(String::as_str);
            let Some((index_path, text)) = self.validated_project_index(project)? else {
                continue;
            };
            items.extend(self.read_project_tasks(project, repo, &index_path, &text)?);
        }

        Ok(items)
    }
}

impl ObsidianPendingWorkStore {
    fn read_project_tasks(
        &self,
        project: &ProjectName,
        repo: Option<&str>,
        index_path: &Path,
        text: &str,
    ) -> Result<Vec<OpenItem>, ObsidianPendingWorkStoreError> {
        let task_files = self.task_files_for_project(&project)?;
        Ok(parse_project_tasks(
            project.as_ref(),
            repo,
            index_path,
            text,
            |id| find_task_file(&task_files, id),
        ))
    }

    pub(super) fn validated_project_index(
        &self,
        project: &ProjectName,
    ) -> Result<Option<(PathBuf, String)>, ObsidianPendingWorkStoreError> {
        let index_path = pwf_core::paths::project_index_path(
            self.config.notes_dir_for(project.as_ref()),
            project.as_ref(),
        );
        let Some(text) = read_text_optional(&index_path) else {
            return Ok(None);
        };
        let actual = parse_project_index_identity(&index_path, &text)?;
        let expected = configured_project_index_identity(&self.config, project)?;
        validate_project_index_identity(&index_path, &actual, &expected)?;
        Ok(Some((index_path, text)))
    }
}

fn find_task_file(
    task_files: &[TaskFile],
    id: &str,
) -> Option<(std::path::PathBuf, String, Option<String>)> {
    task_files
        .iter()
        .find(|task| task.id.as_ref() == id)
        .map(|task| (task.path.clone(), task.markdown.clone(), task.title.clone()))
}
