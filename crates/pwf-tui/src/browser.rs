use std::{collections::BTreeSet, path::PathBuf};

use pwf_models::{
    note::NoteId,
    project::ProjectId,
    task::{TaskId, TaskStatus},
};
use ratatui_textarea::TextArea;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum RecordId {
    Task(TaskId),
    Note(NoteId),
}

impl RecordId {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Task(id) => id.as_ref(),
            Self::Note(id) => id.as_ref(),
        }
    }

    pub fn task(&self) -> Option<&TaskId> {
        match self {
            Self::Task(id) => Some(id),
            Self::Note(_) => None,
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct Project {
    pub id: ProjectId,
    pub title: String,
    pub tasks_path: PathBuf,
}

#[derive(Clone, Debug)]
pub(super) struct Record {
    pub id: RecordId,
    pub project: ProjectId,
    pub title: String,
    pub status: Option<TaskStatus>,
    pub verified: bool,
    pub body: String,
    pub path: PathBuf,
    pub metadata: Vec<(String, String)>,
    pub diagnostic: Option<String>,
    name_folded: String,
    body_folded: String,
}

impl Record {
    pub fn index(&mut self) {
        self.name_folded =
            format!("{} {} {}", self.id.as_str(), self.project, self.title).to_lowercase();
        self.body_folded = self.body.to_lowercase();
    }

    pub fn new(id: RecordId, project: ProjectId, title: String, path: PathBuf) -> Self {
        Self {
            id,
            project,
            title,
            status: None,
            verified: false,
            body: String::new(),
            path,
            metadata: Vec::new(),
            diagnostic: None,
            name_folded: String::new(),
            body_folded: String::new(),
        }
    }

    pub fn bytes(&self) -> usize {
        self.body.len() + self.body_folded.len() + self.name_folded.len() + self.title.len()
    }

    pub fn status_label(&self) -> &'static str {
        match self.status {
            Some(TaskStatus::Active) => "active",
            Some(TaskStatus::Backlog) => "backlog",
            Some(TaskStatus::Done) => "done",
            Some(TaskStatus::Cancelled) => "cancelled",
            None if self.verified => "verified note",
            None => "note",
        }
    }

    pub fn content_match(&self, query: &str) -> Option<(usize, &str)> {
        let sensitive = query.chars().any(char::is_uppercase);
        let folded = query.to_lowercase();
        self.body.lines().enumerate().find(|(_, line)| {
            (sensitive && line.contains(query))
                || (!sensitive && line.to_lowercase().contains(&folded))
        })
    }

    pub fn preview_header_lines(&self) -> usize {
        5 + self.metadata.len() + usize::from(self.diagnostic.is_some())
    }
}

pub(super) struct Snapshot {
    pub projects: Vec<Project>,
    pub records: Vec<Record>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum StatusFilter {
    #[default]
    Active,
    Backlog,
    Done,
    Cancelled,
    All,
}

impl StatusFilter {
    pub fn next(self) -> Self {
        match self {
            Self::Active => Self::Backlog,
            Self::Backlog => Self::Done,
            Self::Done => Self::Cancelled,
            Self::Cancelled => Self::All,
            Self::All => Self::Active,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Backlog => "backlog",
            Self::Done => "done",
            Self::Cancelled => "cancelled",
            Self::All => "all statuses",
        }
    }

    fn accepts(self, status: Option<TaskStatus>) -> bool {
        status.is_none_or(|status| match self {
            Self::Active => status == TaskStatus::Active,
            Self::Backlog => status == TaskStatus::Backlog,
            Self::Done => status == TaskStatus::Done,
            Self::Cancelled => status == TaskStatus::Cancelled,
            Self::All => true,
        })
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum KindFilter {
    #[default]
    All,
    Tasks,
    Notes,
}

impl KindFilter {
    pub fn next(self) -> Self {
        match self {
            Self::All => Self::Tasks,
            Self::Tasks => Self::Notes,
            Self::Notes => Self::All,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "tasks + notes",
            Self::Tasks => "tasks",
            Self::Notes => "notes",
        }
    }

    fn accepts(self, id: &RecordId) -> bool {
        match self {
            Self::All => true,
            Self::Tasks => id.task().is_some(),
            Self::Notes => id.task().is_none(),
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum SearchMode {
    #[default]
    Names,
    Contents,
}

impl SearchMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Names => "names",
            Self::Contents => "saved contents",
        }
    }
}

pub(super) struct Browser {
    pub projects: Vec<Project>,
    pub records: Vec<Record>,
    pub project: Option<ProjectId>,
    pub status: StatusFilter,
    pub kind: KindFilter,
    pub search_mode: SearchMode,
    pub query: TextArea<'static>,
    pub searching: bool,
    pub visible: Vec<usize>,
    pub selected: usize,
    pub preview_scroll: usize,
    pub references: BTreeSet<TaskId>,
}

impl Browser {
    pub fn new(project: Option<ProjectId>) -> Self {
        Self {
            projects: Vec::new(),
            records: Vec::new(),
            project,
            status: StatusFilter::default(),
            kind: KindFilter::default(),
            search_mode: SearchMode::default(),
            query: TextArea::default(),
            searching: false,
            visible: Vec::new(),
            selected: 0,
            preview_scroll: 0,
            references: BTreeSet::new(),
        }
    }

    pub fn replace(&mut self, snapshot: Snapshot) {
        let selected = self.current().map(|record| record.id.clone());
        self.projects = snapshot.projects;
        self.records = snapshot.records;
        self.refilter();
        if let Some(id) = selected
            && let Some(position) = self
                .visible
                .iter()
                .position(|index| self.records[*index].id == id)
        {
            self.selected = position;
            self.reset_preview();
        }
    }

    pub fn refilter(&mut self) {
        let query = self.query.lines().join("\n");
        let folded = query.to_lowercase();
        let case_sensitive = query.chars().any(char::is_uppercase);
        self.visible = self
            .records
            .iter()
            .enumerate()
            .filter(|(_, record)| {
                self.project.as_ref().is_none_or(|id| &record.project == id)
                    && self.status.accepts(record.status)
                    && self.kind.accepts(&record.id)
                    && match self.search_mode {
                        SearchMode::Names if case_sensitive => {
                            format!("{} {} {}", record.id.as_str(), record.project, record.title)
                                .contains(&query)
                        }
                        SearchMode::Contents if case_sensitive => record.body.contains(&query),
                        SearchMode::Names => record.name_folded.contains(&folded),
                        SearchMode::Contents => record.body_folded.contains(&folded),
                    }
            })
            .map(|(index, _)| index)
            .collect();
        self.selected = self.selected.min(self.visible.len().saturating_sub(1));
        self.reset_preview();
    }

    pub fn current(&self) -> Option<&Record> {
        self.visible
            .get(self.selected)
            .map(|index| &self.records[*index])
    }

    pub fn move_selection(&mut self, delta: isize) {
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(self.visible.len().saturating_sub(1));
        self.reset_preview();
    }

    fn reset_preview(&mut self) {
        self.preview_scroll = 0;
        let query = self.query.lines().join(" ");
        if self.search_mode == SearchMode::Contents && !query.is_empty() {
            self.preview_scroll = self
                .current()
                .and_then(|record| {
                    record
                        .content_match(&query)
                        .map(|(line, _)| record.preview_header_lines() + line)
                })
                .unwrap_or(0);
        }
    }

    pub fn scroll_preview(&mut self, delta: isize) {
        let last = self.current().map_or(0, |record| {
            (record.preview_header_lines() + record.body.lines().count()).saturating_sub(1)
        });
        self.preview_scroll = self.preview_scroll.saturating_add_signed(delta).min(last);
    }

    pub fn toggle_reference(&mut self) {
        let Some(id) = self.current().and_then(|record| record.id.task()).cloned() else {
            return;
        };
        if !self.references.remove(&id) {
            self.references.insert(id);
        }
    }

    pub fn reference_text(&self) -> Option<String> {
        let ids = if self.references.is_empty() {
            vec![self.current()?.id.task()?]
        } else {
            self.references.iter().collect()
        };
        Some(
            ids.iter()
                .map(|id| format!("[[{id}]]"))
                .collect::<Vec<_>>()
                .join(" "),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::snapshot;

    #[test]
    fn contents_search_and_status_filter_keep_notes_visible() {
        let mut browser = Browser::new(None);
        browser.replace(snapshot());
        browser.search_mode = SearchMode::Contents;
        browser.query.insert_str("saved markdown");
        browser.refilter();
        assert_eq!(browser.visible.len(), 2);
        browser.status = StatusFilter::Done;
        browser.refilter();
        assert_eq!(browser.current().unwrap().title, "Keyboard notes");
        browser.query = TextArea::from(["saved MARKDOWN"]);
        browser.refilter();
        assert_eq!(browser.visible, Vec::<usize>::new());
    }

    #[test]
    fn reference_selection_survives_filters_and_refresh() {
        let mut browser = Browser::new(None);
        browser.replace(snapshot());
        browser.toggle_reference();
        browser.kind = KindFilter::Notes;
        browser.refilter();
        browser.toggle_reference();
        browser.replace(snapshot());
        assert_eq!(browser.reference_text().as_deref(), Some("[[PWF-0007]]"));
        browser.references.clear();
        assert!(browser.reference_text().is_none());
    }
}
