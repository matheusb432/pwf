use std::{path::Path, sync::LazyLock};

use pwf_application::{
    AddItemSpec, CancelItemSpec, ClosedItem, ClosedItemAction, CompleteItemSpec,
    PendingWorkWriteStore, ReopenedItem, UpdateItemSpec,
};
use pwf_domain::pending_work::{AddedItem, ParsePrereqsError, Prereqs, RemovedItem, UpdatedItem};
use regex::Regex;

use super::{
    ObsidianPendingWorkStore, ObsidianPendingWorkStoreError,
    fs::{
        read_index, read_item_file, read_text_or_default, write_add_index_file,
        write_add_item_file, write_index, write_item_file,
    },
    status::CloseItemSpec,
};
use crate::obsidian::{
    index_text::{
        IndexSection, add_link_to_index, add_section_block, remove_index_link, section_exists,
    },
    note_text::{
        WorkItemFields, append_lanes_text, append_report_block_text, inferred_title,
        normalize_title, note_body, replace_body, replace_title, set_commits_text, set_effort_text,
        set_prereq_text, work_item_content,
    },
};

static PREREQ_VALUE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[([A-Z]{2,4}-\d{4})").expect("valid prereq regex"));

impl PendingWorkWriteStore for ObsidianPendingWorkStore {
    type Error = ObsidianPendingWorkStoreError;

    fn add_item(&self, spec: AddItemSpec) -> Result<AddedItem, Self::Error> {
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
        let id = pwf_core::id::next_id(&[&dir, &dir.join(super::fs::ARCHIVE_DIR)], key);
        let item_path = dir.join(format!("{id}.md"));
        let body = note_body(&spec.prompt);
        let content = work_item_content(WorkItemFields {
            title: &session,
            project: &spec.project_name,
            prompt: &body,
            status: "active",
            created: &spec.created,
            completed: None,
            prereq: spec.prereq.as_deref(),
            effort: spec.effort,
        });
        write_add_item_file(&item_path, &content)?;

        let index = pwf_core::paths::project_index_path(
            self.config.notes_dir_for(&spec.project_name),
            &spec.project_name,
        );
        let index_dir = index.parent().unwrap_or(Path::new("."));
        if !index_dir.exists() {
            std::fs::create_dir_all(index_dir)
                .map_err(|source| ObsidianPendingWorkStoreError::CreateIndexDir { source })?;
        }
        let existing = if index.exists() {
            read_text_or_default(&index)
        } else {
            String::new()
        };
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

    fn update_item(&self, spec: UpdateItemSpec) -> Result<UpdatedItem, Self::Error> {
        let edits_body = spec.prompt.is_some()
            || spec.title.is_some()
            || !spec.prereq.is_empty()
            || spec.clear_prereq
            || spec.append.is_some()
            || spec.effort.is_some();
        if !edits_body && spec.commits.is_none() && spec.append_report.is_none() {
            return Err(ObsidianPendingWorkStoreError::NothingToUpdate);
        }

        match self.find_pending_item(&spec.id) {
            Ok(item) => self.update_open_item(&spec, &item),
            Err(ObsidianPendingWorkStoreError::ItemNotFound { id }) => {
                match self.find_item_note_file(&id) {
                    Some(_) if edits_body => {
                        Err(ObsidianPendingWorkStoreError::ClosedItemAmendOnly { id })
                    }
                    Some(path) => Self::amend_closed_item(
                        &path,
                        spec.commits.as_deref(),
                        spec.append_report.as_deref(),
                        &id,
                    ),
                    None => Err(ObsidianPendingWorkStoreError::ItemNotFound { id }),
                }
            }
            Err(other) => Err(other),
        }
    }

    fn remove_item(&self, id: &str) -> Result<RemovedItem, Self::Error> {
        let item = self.find_pending_item(id)?;
        let item_file = item
            .item_file
            .as_deref()
            .ok_or(ObsidianPendingWorkStoreError::RemoveRequiresFileModel)?;
        let item_path = Path::new(item_file);
        if !item_path.exists() {
            return Err(ObsidianPendingWorkStoreError::WorkItemNoteMissing {
                path: item_path.to_path_buf(),
            });
        }

        let index_path = pwf_core::paths::project_index_path(
            self.config.notes_dir_for(&item.project),
            &item.project,
        );
        let index_content = read_index(&index_path)?;
        let removed = remove_index_link(&index_content, &item.id);
        if removed == index_content {
            return Err(ObsidianPendingWorkStoreError::IndexLinkNotFound { id: item.id });
        }
        write_index(&index_path, &removed)?;
        std::fs::remove_file(item_path)
            .map_err(|source| ObsidianPendingWorkStoreError::RemoveItemFile { source })?;

        Ok(RemovedItem {
            id: item.id,
            project: item.project,
            title: item.session,
            deleted_path: item_path.to_path_buf(),
            unlinked: index_path.display().to_string(),
        })
    }

    fn complete_item(&self, spec: CompleteItemSpec) -> Result<ClosedItem, Self::Error> {
        self.close_item(
            &CloseItemSpec {
                id: spec.id,
                completed: spec.completed,
                report: spec.report,
                commits: spec.commits,
            },
            ClosedItemAction::Done,
        )
    }

    fn cancel_item(&self, spec: CancelItemSpec) -> Result<ClosedItem, Self::Error> {
        if spec.report.trim().is_empty() {
            return Err(ObsidianPendingWorkStoreError::EmptyReport);
        }
        self.close_item(
            &CloseItemSpec {
                id: spec.id,
                completed: spec.completed,
                report: Some(spec.report),
                commits: spec.commits,
            },
            ClosedItemAction::Cancelled,
        )
    }

    fn reopen_item(&self, id: &str) -> Result<ReopenedItem, Self::Error> {
        self.reopen_item_impl(id)
    }
}

impl ObsidianPendingWorkStore {
    fn update_open_item(
        &self,
        spec: &UpdateItemSpec,
        item: &pwf_domain::pending_work::OpenItem,
    ) -> Result<UpdatedItem, ObsidianPendingWorkStoreError> {
        let item_file = item
            .item_file
            .as_deref()
            .ok_or(ObsidianPendingWorkStoreError::UpdateRequiresFileModel)?;
        let item_path = Path::new(item_file);
        let mut content = read_item_file(item_path)?;

        let new_title = spec
            .title
            .as_deref()
            .map_or_else(|| item.session.clone(), normalize_title);
        if spec.title.is_some() {
            content = replace_title(&content, &new_title);
        }
        if let Some(prompt) = spec.prompt.as_deref() {
            content = replace_body(&content, &note_body(prompt));
        }
        if let Some(append) = spec.append.as_deref() {
            content = append_lanes_text(&content, append)
                .ok_or(ObsidianPendingWorkStoreError::EmptyAppend)?;
        }
        if spec.clear_prereq {
            content = set_prereq_text(&content, None);
        } else if !spec.prereq.is_empty() {
            let merged = self.append_prereq_frontmatter(item.prereq.as_deref(), &spec.prereq)?;
            content = set_prereq_text(&content, Some(&merged));
        }
        if let Some(commits) = spec.commits.as_deref() {
            content = set_commits_text(&content, Some(commits));
        }
        if let Some(effort) = spec.effort {
            content = set_effort_text(&content, Some(effort));
        }
        if let Some(report) = spec.append_report.as_deref() {
            content = append_report_block_text(&content, report)
                .ok_or(ObsidianPendingWorkStoreError::EmptyReport)?;
        }
        write_item_file(item_path, &content)?;

        Ok(UpdatedItem::OpenItemEdit {
            id: item.id.clone(),
            project: item.project.clone(),
            title: new_title,
        })
    }

    fn amend_closed_item(
        path: &Path,
        commits: Option<&str>,
        append_report: Option<&str>,
        id: &str,
    ) -> Result<UpdatedItem, ObsidianPendingWorkStoreError> {
        let mut content = read_item_file(path)?;
        let mut changes = Vec::new();
        if let Some(commits) = commits {
            content = set_commits_text(&content, Some(commits));
            changes.push(format!("commits: {commits}"));
        }
        if let Some(report) = append_report {
            content = append_report_block_text(&content, report)
                .ok_or(ObsidianPendingWorkStoreError::EmptyReport)?;
            changes.push("report appended".to_string());
        }
        write_item_file(path, &content)?;
        Ok(UpdatedItem::Changed {
            id: id.to_string(),
            changes,
        })
    }

    fn append_prereq_frontmatter(
        &self,
        existing: Option<&str>,
        values: &[String],
    ) -> Result<String, ObsidianPendingWorkStoreError> {
        let mut ids: Vec<String> = existing
            .into_iter()
            .flat_map(|value| {
                PREREQ_VALUE_RE
                    .captures_iter(value)
                    .map(|c| c[1].to_string())
            })
            .collect();
        let prereqs = Prereqs::parse_values(values).map_err(map_parse_prereqs_error)?;
        let missing: Vec<String> = prereqs
            .ids()
            .into_iter()
            .filter(|id| self.read_status(id).is_none())
            .map(str::to_string)
            .collect();
        if !missing.is_empty() {
            return Err(ObsidianPendingWorkStoreError::UnknownPrereqIds { ids: missing });
        }
        for id in prereqs.ids() {
            if !ids.iter().any(|existing| existing == id) {
                ids.push(id.to_string());
            }
        }
        Ok(ids
            .iter()
            .map(|id| format!("[[{id}]]"))
            .collect::<Vec<_>>()
            .join(", "))
    }
}

fn map_parse_prereqs_error(error: ParsePrereqsError) -> ObsidianPendingWorkStoreError {
    match error {
        ParsePrereqsError::MissingId => ObsidianPendingWorkStoreError::MissingPrereqId,
        ParsePrereqsError::InvalidId { raw } => {
            ObsidianPendingWorkStoreError::InvalidPrereqId { raw }
        }
    }
}
