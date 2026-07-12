use std::{fmt::Write as _, path::Path, sync::LazyLock};

use pwf_application::{ClosedItem, ClosedItemAction, ReopenedItem, StatusTransitionDiagnostics};
use pwf_domain::pending_work::{OpenItem, ProjectName, WorkItemStatus};
use regex::Regex;

use super::{
    ObsidianPendingWorkStore, ObsidianPendingWorkStoreError,
    fs::{line_start_index, read_index, read_item_file, write_index, write_item_file},
    lookup::normalize_lookup_id,
};
use crate::obsidian::{
    done_queue,
    identity::parse_task_identity_if_task,
    index_text::add_link_to_index,
    note_frontmatter::{reopen_status_text, set_commits_text, set_status_text},
    note_text::append_report_text,
};

static DATE_STAMP_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"✅\s*(\d{4}-\d{2}-\d{2})").expect("valid date stamp regex"));

pub(super) struct CloseItemSpec {
    pub(super) id: String,
    pub(super) completed: String,
    pub(super) commits: Option<String>,
    pub(super) kind: CloseItemKind,
}

pub(super) enum CloseItemKind {
    Done { report: Option<String> },
    Cancelled { report: String },
}

impl CloseItemKind {
    fn action(&self) -> ClosedItemAction {
        match self {
            Self::Done { .. } => ClosedItemAction::Done,
            Self::Cancelled { .. } => ClosedItemAction::Cancelled,
        }
    }

    fn status(&self) -> WorkItemStatus {
        match self {
            Self::Done { .. } => WorkItemStatus::Done,
            Self::Cancelled { .. } => WorkItemStatus::Cancelled,
        }
    }

    fn report(&self) -> Option<&str> {
        match self {
            Self::Done { report } => report.as_deref(),
            Self::Cancelled { report } => Some(report),
        }
    }
}

impl ObsidianPendingWorkStore {
    pub(super) fn reopen_item_impl(
        &self,
        id: &str,
    ) -> Result<ReopenedItem, ObsidianPendingWorkStoreError> {
        let (project, note_path) = self.find_item_note_with_project(id)?.ok_or_else(|| {
            ObsidianPendingWorkStoreError::ItemNotFound {
                id: normalize_lookup_id(id),
            }
        })?;
        let content = read_item_file(&note_path)?;
        let canonical = parse_task_identity_if_task(&note_path, &content)?
            .ok_or_else(|| ObsidianPendingWorkStoreError::MissingTaskId {
                path: note_path.clone(),
            })?
            .as_ref()
            .to_string();

        if pwf_core::frontmatter::parse(&content)
            .frontmatter
            .get("status")
            .map(String::as_str)
            == Some(WorkItemStatus::Active.as_frontmatter_str())
        {
            return Ok(ReopenedItem {
                id: canonical,
                project,
                already_active: true,
            });
        }

        let updated = set_commits_text(&reopen_status_text(&content), None);
        write_item_file(&note_path, &updated)?;

        let project_name =
            ProjectName::try_new(&project).expect("resolved project name is non-empty");
        if let Some((index_path, index)) = self.validated_project_index(&project_name)? {
            let restored = match done_queue::reopen_done_link(&index, &canonical) {
                Some(content) => content,
                None => add_link_to_index(&index, &format!("- [ ] [[{canonical}]]")),
            };
            write_index(&index_path, &restored)?;
        }

        Ok(ReopenedItem {
            id: canonical,
            project,
            already_active: false,
        })
    }

    pub(super) fn close_item(
        &self,
        spec: &CloseItemSpec,
    ) -> Result<ClosedItem, ObsidianPendingWorkStoreError> {
        let item = self.find_pending_item(&spec.id)?;
        if item.item_file.is_some() {
            self.close_file_model_item(spec, &item)
        } else {
            Self::close_legacy_item(spec, &item)
        }
    }

    fn close_file_model_item(
        &self,
        spec: &CloseItemSpec,
        item: &OpenItem,
    ) -> Result<ClosedItem, ObsidianPendingWorkStoreError> {
        let item_file = item
            .item_file
            .as_deref()
            .ok_or(ObsidianPendingWorkStoreError::UpdateRequiresFileModel)?;
        let item_path = Path::new(item_file);
        let mut content = read_item_file(item_path)?;
        if let Some(report) = spec.kind.report() {
            content = append_report_text(&content, report)
                .ok_or(ObsidianPendingWorkStoreError::EmptyReport)?;
        }
        if let Some(commits) = spec.commits.as_deref() {
            content = set_commits_text(&content, Some(commits));
        }
        content = set_status_text(&content, spec.kind.status(), &spec.completed);
        write_item_file(item_path, &content)?;

        let diagnostics = self.rotate_done_queue(item, &spec.completed)?;

        Ok(ClosedItem {
            id: item.id.clone(),
            project: item.project.clone(),
            title: item.session.clone(),
            action: spec.kind.action(),
            diagnostics,
        })
    }

    fn rotate_done_queue(
        &self,
        item: &OpenItem,
        completed: &str,
    ) -> Result<StatusTransitionDiagnostics, ObsidianPendingWorkStoreError> {
        let notes_dir = self.config.notes_dir_for(&item.project);
        let index_path = pwf_core::paths::project_index_path(notes_dir, &item.project);
        if !index_path.exists() {
            return Ok(StatusTransitionDiagnostics::none());
        }
        let index = read_index(&index_path)?;
        // TODO: refactor coupling and unintuitive function name. 'rotate_done_queue' should not be responsible for marking the item done
        let queue = done_queue::mark_done(&index, &item.id, completed);
        write_index(&index_path, &queue.content)?;

        Ok(StatusTransitionDiagnostics {
            futuro_renamed_project: queue.futuro_renamed.then(|| item.project.clone()),
            evicted_ids: queue.evicted,
        })
    }

    fn close_legacy_item(
        spec: &CloseItemSpec,
        item: &OpenItem,
    ) -> Result<ClosedItem, ObsidianPendingWorkStoreError> {
        let note_path = Path::new(&item.note);
        let content = read_index(note_path)?;
        let marker_index = line_start_index(&content, item.line).ok_or_else(|| {
            ObsidianPendingWorkStoreError::ExpectedOpenTaskMarker {
                note: item.note.clone(),
                line: item.line,
            }
        })?;
        let marker = content
            .get(marker_index..marker_index + 5)
            .unwrap_or_default();
        if marker != "- [ ]" {
            return Err(ObsidianPendingWorkStoreError::ExpectedOpenTaskMarker {
                note: item.note.clone(),
                line: item.line,
            });
        }
        let line_end = content[marker_index..]
            .find(['\r', '\n'])
            .map_or(content.len(), |index| marker_index + index);
        let line_text = &content[marker_index..line_end];
        let mut checked_line = format!("- [x]{}", &line_text[5..]);
        if !DATE_STAMP_RE.is_match(&checked_line) {
            let _ = write!(checked_line, " ✅ {}", spec.completed);
        }
        let updated = format!(
            "{}{}{}",
            &content[..marker_index],
            checked_line,
            &content[line_end..]
        );
        write_index(note_path, &updated)?;

        Ok(ClosedItem {
            id: item.id.clone(),
            project: item.project.clone(),
            title: item.session.clone(),
            action: spec.kind.action(),
            diagnostics: StatusTransitionDiagnostics::none(),
        })
    }
}
