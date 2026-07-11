use std::{path::Path, sync::LazyLock};

use pwf_application::UpdateItemSpec;
use pwf_domain::pending_work::{ParsePrereqsError, Prereqs, Tags, UpdatedItem};
use regex::Regex;

use super::{
    ObsidianPendingWorkStore, ObsidianPendingWorkStoreError,
    fs::{read_item_file, write_item_file},
};
use crate::obsidian::{
    note_frontmatter::{set_commits_text, set_effort_text, set_prereq_text, set_tags_text},
    note_text::{
        append_lanes_text, append_report_block_text, normalize_title, note_body, replace_body,
        replace_title,
    },
};

static PREREQ_VALUE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[([A-Z]{2,4}-\d{4})").expect("valid prereq regex"));

impl ObsidianPendingWorkStore {
    pub(super) fn update_item_impl(
        &self,
        spec: &UpdateItemSpec,
    ) -> Result<UpdatedItem, ObsidianPendingWorkStoreError> {
        let edits_body = spec.prompt.is_some()
            || spec.title.is_some()
            || !spec.prereq.is_empty()
            || spec.clear_prereq
            || spec.append.is_some()
            || spec.effort.is_some()
            || spec.tags.is_some()
            || spec.tags_clear;
        if !edits_body && spec.commits.is_none() && spec.append_report.is_none() {
            return Err(ObsidianPendingWorkStoreError::NothingToUpdate);
        }

        match self.find_pending_item(&spec.id) {
            Ok(item) => self.update_open_item(spec, &item),
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
        if spec.tags_clear {
            content = set_tags_text(&content, None);
        }
        if let Some(appended) = spec.tags.as_ref() {
            let tags = if spec.tags_clear {
                appended.clone()
            } else if let Some(existing) = item.tags.as_deref() {
                Tags::parse_frontmatter(existing)
                    .map_err(
                        |error| ObsidianPendingWorkStoreError::InvalidTagsFrontmatter {
                            id: item.id.clone(),
                            raw: error.raw().to_string(),
                        },
                    )?
                    .merged(appended)
            } else {
                appended.clone()
            };
            content = set_tags_text(&content, Some(&tags));
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
                    .map(|captures| captures[1].to_string())
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
