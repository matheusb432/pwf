use std::path::Path;

use pwf_application::AddItemSpec;
use pwf_domain::pending_work::{AddedItem, ProjectName};

use super::{
    ObsidianPendingWorkStore, ObsidianPendingWorkStoreError,
    fs::{read_text_or_default, write_add_index_file, write_add_item_file},
};
use crate::obsidian::{
    identity::{
        configured_project_index_identity, new_project_index_content, parse_project_index_identity,
        validate_project_index_identity,
    },
    index_text::{IndexSection, add_link_to_index, add_section_block, section_exists},
    note_frontmatter::{NewWorkItemFields, new_work_item_content},
    note_text::{inferred_title, normalize_title, note_body},
};

impl ObsidianPendingWorkStore {
    pub(super) fn add_item_impl(
        &self,
        spec: AddItemSpec,
    ) -> Result<AddedItem, ObsidianPendingWorkStoreError> {
        let repo = self
            .config
            .projects
            .get(&spec.project_name)
            .map_or("", String::as_str);
        if repo.trim().is_empty() {
            return Err(ObsidianPendingWorkStoreError::ProjectNotMappedToRepo {
                project: spec.project_name,
            });
        }

        let session = match spec.title.as_deref() {
            Some(title) if !title.trim().is_empty() => normalize_title(title),
            _ => inferred_title(&spec.prompt),
        };
        let key =
            pwf_core::paths::project_key(&self.config, &spec.project_name).ok_or_else(|| {
                ObsidianPendingWorkStoreError::ProjectMissingPrefix {
                    project: spec.project_name.clone(),
                }
            })?;
        let dir = pwf_core::paths::project_dir(
            self.config.notes_dir_for(&spec.project_name),
            &spec.project_name,
        );
        if !dir.exists() {
            std::fs::create_dir_all(&dir)
                .map_err(|source| ObsidianPendingWorkStoreError::CreateProjectDir { source })?;
        }
        let index = pwf_core::paths::project_index_path(
            self.config.notes_dir_for(&spec.project_name),
            &spec.project_name,
        );
        let index_dir = index.parent().unwrap_or(Path::new("."));
        if !index_dir.exists() {
            std::fs::create_dir_all(index_dir)
                .map_err(|source| ObsidianPendingWorkStoreError::CreateIndexDir { source })?;
        }
        let project_name =
            ProjectName::try_new(&spec.project_name).expect("resolved project is non-empty");
        let identity = configured_project_index_identity(&self.config, &project_name)?;
        let existing = if index.is_file() {
            let content = read_text_or_default(&index);
            let actual = parse_project_index_identity(&index, &content)?;
            validate_project_index_identity(&index, &actual, &identity)?;
            content
        } else {
            new_project_index_content(&identity)
        };
        let id = self.next_task_id(&project_name, key)?;
        let item_path = dir.join(format!("{id}.md"));
        let body = note_body(&spec.prompt);
        let content = new_work_item_content(NewWorkItemFields {
            id: &id,
            title: &session,
            project: &spec.project_name,
            prompt: &body,
            created: &spec.created,
            prereq: spec.prereq.as_deref(),
            effort: spec.effort,
            tags: spec.tags.as_ref(),
        });
        write_add_item_file(&item_path, &content)?;
        let link = format!("- [ ] [[{id}]]");
        let (updated, created_section) =
            if let Some(section) = spec.section.as_deref().and_then(IndexSection::parse) {
                let created_section = (!section_exists(&existing, section))
                    .then(|| spec.section.clone())
                    .flatten();
                (
                    add_section_block(&existing, &format!("{link}\n"), section),
                    created_section,
                )
            } else {
                (add_link_to_index(&existing, &link), None)
            };
        write_add_index_file(
            &index,
            &updated,
            &spec.project_name,
            created_section.as_deref(),
        )?;

        Ok(AddedItem {
            id,
            project: spec.project_name,
            title: session,
            note_path: item_path,
            created_section,
        })
    }
}
