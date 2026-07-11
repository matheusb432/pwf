use std::path::Path;

use pwf_application::PendingWorkReadStore;
use pwf_domain::pending_work::OpenItem;

use super::{
    ObsidianPendingWorkStore, ObsidianPendingWorkStoreError,
    fs::{read_text_optional, read_text_or_default},
    read_parser::parse_project_tasks,
};

impl PendingWorkReadStore for ObsidianPendingWorkStore {
    type Error = ObsidianPendingWorkStoreError;

    fn open_items(&self, only_project: Option<&str>) -> Result<Vec<OpenItem>, Self::Error> {
        if !Path::new(&self.config.notes_dir).exists() {
            return Err(ObsidianPendingWorkStoreError::NotesDirectoryNotFound {
                path: self.config.notes_dir.clone(),
            });
        }

        let project_names: Vec<String> = if let Some(project) = only_project {
            vec![project.to_string()]
        } else {
            self.config.projects.keys().cloned().collect()
        };

        let mut items = Vec::new();
        for project in project_names {
            let index_path = Path::new(self.config.notes_dir_for(&project))
                .join(&project)
                .join(format!("{project}.md"));
            if !index_path.exists() {
                continue;
            }

            let repo = self.config.projects.get(&project).map(String::as_str);
            items.extend(read_project_tasks(&project, repo, &index_path));
        }

        Ok(items)
    }
}

fn read_project_tasks(project: &str, repo: Option<&str>, index_path: &Path) -> Vec<OpenItem> {
    let Some(text) = read_text_optional(index_path) else {
        return Vec::new();
    };
    parse_project_tasks(project, repo, index_path, &text, |item_path| {
        item_path.exists().then(|| read_text_or_default(item_path))
    })
}
