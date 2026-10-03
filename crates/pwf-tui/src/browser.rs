use std::{collections::BTreeSet, path::PathBuf};

use aho_corasick::AhoCorasick;
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
pub(super) enum ProjectScope {
    All,
    Project(ProjectId),
    Directory(PathBuf),
}

impl ProjectScope {
    pub fn project(&self) -> Option<&ProjectId> {
        match self {
            Self::Project(id) => Some(id),
            Self::All | Self::Directory(_) => None,
        }
    }
}

impl From<Option<ProjectId>> for ProjectScope {
    fn from(project: Option<ProjectId>) -> Self {
        project.map_or(Self::All, Self::Project)
    }
}

#[derive(Clone, Debug)]
pub(super) struct Record {
    pub id: RecordId,
    pub project: ProjectId,
    pub title: String,
    pub status: Option<TaskStatus>,
    pub verified: bool,
    body: String,
    pub path: PathBuf,
    pub metadata: Vec<(String, String)>,
    pub diagnostic: Option<String>,
    id_folded: String,
    body_unicode_folds_to_ascii: bool,
}

impl Record {
    pub fn set_body(&mut self, body: String) {
        self.body = body;
        self.body_unicode_folds_to_ascii = !self.body.is_ascii()
            && self.body.chars().any(|character| {
                !character.is_ascii() && character.to_lowercase().any(|folded| folded.is_ascii())
            });
    }

    pub fn body(&self) -> &str {
        &self.body
    }

    pub fn new(id: RecordId, project: ProjectId, title: String, path: PathBuf) -> Self {
        let id_folded = id.as_str().to_ascii_lowercase();
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
            id_folded,
            body_unicode_folds_to_ascii: false,
        }
    }

    pub fn bytes(&self) -> usize {
        self.body.len() + self.id_folded.len() + self.title.len()
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

    pub fn preview_header_lines(&self) -> usize {
        4 + self.metadata.len() + usize::from(self.diagnostic.is_some())
    }
}

struct ContentQuery {
    text: String,
    sensitive: bool,
    matcher: Option<AhoCorasick>,
}

impl ContentQuery {
    fn new(query: &str) -> Self {
        let sensitive = query.chars().any(char::is_uppercase);
        let text = if sensitive {
            query.to_string()
        } else {
            query.to_lowercase()
        };
        let matcher = (!sensitive)
            .then(|| {
                AhoCorasick::builder()
                    .ascii_case_insensitive(true)
                    .build([&text])
                    .ok()
            })
            .flatten();
        Self {
            text,
            sensitive,
            matcher,
        }
    }

    fn matches(&self, text: &str, unicode_folds_to_ascii: bool) -> bool {
        if self.sensitive {
            return text.contains(&self.text);
        }
        if let Some(matcher) = &self.matcher {
            if matcher.is_match(text) {
                return true;
            }
            if self.text.is_ascii() && !unicode_folds_to_ascii {
                return false;
            }
        }
        text.to_lowercase().contains(&self.text)
    }

    fn first_match_line(&self, text: &str, unicode_folds_to_ascii: bool) -> Option<usize> {
        if self.text.contains(['\r', '\n']) {
            return text
                .lines()
                .position(|line| self.matches(line, unicode_folds_to_ascii));
        }
        let folded;
        let (text, offset) = if self.sensitive {
            (text, text.find(&self.text))
        } else if self.text.is_ascii()
            && !unicode_folds_to_ascii
            && let Some(matcher) = &self.matcher
        {
            (text, matcher.find(text).map(|matched| matched.start()))
        } else {
            folded = text.to_lowercase();
            let offset = folded.find(&self.text);
            (folded.as_str(), offset)
        };
        offset.map(|offset| text[..offset].matches('\n').count())
    }
}

#[derive(Clone, Copy)]
enum ContentMatch {
    Unchecked,
    Missing,
    Line(usize),
}

pub(super) struct Snapshot {
    pub project: Option<ProjectId>,
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
    Ids,
    Contents,
}

impl SearchMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ids => "IDs",
            Self::Contents => "saved contents",
        }
    }
}

pub(super) struct Browser {
    pub projects: Vec<Project>,
    pub records: Vec<Record>,
    pub project_scope: ProjectScope,
    pub status: StatusFilter,
    pub kind: KindFilter,
    pub search_mode: SearchMode,
    pub query: TextArea<'static>,
    pub searching: bool,
    pub visible: Vec<usize>,
    pub selected: usize,
    pub preview_scroll: usize,
    pub references: BTreeSet<TaskId>,
    content_query: Option<ContentQuery>,
    content_matches: Vec<ContentMatch>,
}

impl Browser {
    pub fn new(project_scope: ProjectScope) -> Self {
        Self {
            projects: Vec::new(),
            records: Vec::new(),
            project_scope,
            status: StatusFilter::default(),
            kind: KindFilter::default(),
            search_mode: SearchMode::default(),
            query: TextArea::default(),
            searching: false,
            visible: Vec::new(),
            selected: 0,
            preview_scroll: 0,
            references: BTreeSet::new(),
            content_query: None,
            content_matches: Vec::new(),
        }
    }

    pub fn replace(&mut self, snapshot: Snapshot) {
        let selected = self.current().map(|record| record.id.clone());
        self.project_scope = snapshot.project.into();
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
        let task_id = query.parse::<TaskId>().ok();
        self.content_query = (self.search_mode == SearchMode::Contents && !query.is_empty())
            .then(|| ContentQuery::new(&query));
        self.content_matches.clear();
        self.content_matches
            .resize(self.records.len(), ContentMatch::Unchecked);
        self.visible = self
            .records
            .iter()
            .enumerate()
            .filter(|(_, record)| {
                self.project_scope
                    .project()
                    .is_none_or(|id| &record.project == id)
                    && self.status.accepts(record.status)
                    && self.kind.accepts(&record.id)
                    && match self.search_mode {
                        SearchMode::Ids => {
                            let mut candidate = record.id_folded.bytes();
                            (task_id.is_none() || record.id.task().is_some())
                                && folded
                                    .trim()
                                    .bytes()
                                    .all(|character| candidate.any(|next| next == character))
                        }
                        SearchMode::Contents => self.content_query.as_ref().is_none_or(|query| {
                            query.matches(&record.body, record.body_unicode_folds_to_ascii)
                        }),
                    }
            })
            .map(|(index, _)| index)
            .collect();
        if self.search_mode == SearchMode::Ids
            && let Some(id) = &task_id
        {
            self.visible
                .sort_by_key(|index| self.records[*index].id.task() != Some(id));
        }
        self.selected = self.selected.min(self.visible.len().saturating_sub(1));
        self.reset_preview();
    }

    pub fn current(&self) -> Option<&Record> {
        self.visible
            .get(self.selected)
            .map(|index| &self.records[*index])
    }

    pub fn content_match_line(&mut self, index: usize) -> Option<usize> {
        let query = self.content_query.as_ref()?;
        if matches!(self.content_matches[index], ContentMatch::Unchecked) {
            let record = &self.records[index];
            self.content_matches[index] = query
                .first_match_line(&record.body, record.body_unicode_folds_to_ascii)
                .map_or(ContentMatch::Missing, ContentMatch::Line);
        }
        match self.content_matches[index] {
            ContentMatch::Line(line) => Some(line),
            ContentMatch::Missing | ContentMatch::Unchecked => None,
        }
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
        if let Some(index) = self.visible.get(self.selected).copied()
            && let Some(line) = self.content_match_line(index)
        {
            self.preview_scroll = self.records[index].preview_header_lines() + line;
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
    fn id_search_prioritizes_compact_tasks_and_excludes_record_text() {
        let mut browser = Browser::new(ProjectScope::All);
        for raw in ["ALP-0012", "ALP-0021", "ALP-0001", "BET-0009"] {
            let id: TaskId = raw.parse().unwrap();
            let mut record = Record::new(
                RecordId::Task(id.clone()),
                id.project_id().clone(),
                "alp1 title-only-needle".into(),
                PathBuf::from(format!("/tasks/{id}.md")),
            );
            record.status = Some(TaskStatus::Active);
            record.set_body("alp1 body-only-needle".into());
            browser.records.push(record);
        }
        let note = Record::new(
            RecordId::Note(NoteId::try_new("ALP-NOTE-0001").unwrap()),
            "ALP".parse().unwrap(),
            "alp1 note title".into(),
            "/tasks/ALP-NOTE-0001.md".into(),
        );
        browser.records.push(note);

        for query in ["alp1", "AlP1", "alp-1", "alp0001", "ALP-0001", " alp1 "] {
            browser.query = TextArea::from([query]);
            browser.refilter();
            assert_eq!(
                browser.current().unwrap().id.as_str(),
                "ALP-0001",
                "{query}"
            );
            assert!(browser.visible.iter().all(|index| {
                let record = &browser.records[*index];
                record.project.as_ref() == "ALP" && record.id.task().is_some()
            }));
        }
        browser.query = TextArea::from(["alp1"]);
        browser.refilter();
        assert_eq!(browser.visible, [2, 0, 1]);
        for query in ["title-only-needle", "body-only-needle"] {
            browser.query = TextArea::from([query]);
            browser.refilter();
            assert_eq!(browser.visible, Vec::<usize>::new());
        }
        browser.query = TextArea::from(["12"]);
        browser.refilter();
        assert_eq!(browser.current().unwrap().id.as_str(), "ALP-0012");
        browser.kind = KindFilter::Notes;
        browser.query = TextArea::from(["alp-note"]);
        browser.refilter();
        assert_eq!(browser.current().unwrap().id.as_str(), "ALP-NOTE-0001");
    }

    #[test]
    fn contents_search_and_status_filter_keep_notes_visible() {
        let mut browser = Browser::new(ProjectScope::All);
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
    fn contents_search_preserves_unicode_lowercasing_and_smart_case() {
        let bodies = [
            "An ASCII Needle\nnext line",
            "İstanbul and \u{212a}elvin\r\nAÇÃO",
            "İ",
            "\u{212a}",
            "İ first\nistanbul later",
            "\u{212a}elvin first\nkelvin later",
            "AÇÃO first\nação later",
            "ΟΣ",
            "ΟΣΑ",
            "ΟΣ ΟΣΑ οσ ος\nΣ",
            "conteúdo útil\nUppercase Title",
            "first\r\nneedle\r\nlast\r",
        ];
        let queries = [
            "needle",
            "Needle",
            "NEEDLE",
            "i",
            "i\u{307}",
            "istanbul",
            "kelvin",
            "ação",
            "AÇÃO",
            "οσ",
            "ος",
            "σ",
            "conteúdo",
            "CONTEÚDO",
            "absent",
            "needle\nnext",
            "needle\r",
        ];
        for body in bodies {
            let mut saved = snapshot();
            saved.records.truncate(1);
            saved.records[0].set_body(body.into());
            let mut browser = Browser::new(ProjectScope::All);
            browser.replace(saved);
            browser.search_mode = SearchMode::Contents;
            for query in queries {
                browser.query = TextArea::from(query.split('\n'));
                browser.refilter();
                let matches = |text: &str| contents_match_reference(text, query);
                assert_eq!(
                    !browser.visible.is_empty(),
                    matches(body),
                    "{body:?} / {query:?}"
                );
                assert_eq!(
                    browser.content_match_line(0),
                    body.lines().position(matches),
                    "snippet {body:?} / {query:?}"
                );
            }
        }
    }

    fn contents_match_reference(text: &str, query: &str) -> bool {
        if query.chars().any(char::is_uppercase) {
            text.contains(query)
        } else {
            text.to_lowercase().contains(&query.to_lowercase())
        }
    }

    #[test]
    fn contents_snippets_follow_query_changes_and_refreshed_bodies() {
        let mut browser = Browser::new(ProjectScope::All);
        let mut saved = snapshot();
        saved.records[0].set_body("first needle\nsecond marker".into());
        browser.replace(saved);
        browser.search_mode = SearchMode::Contents;
        for (query, line) in [("needle", 0), ("marker", 1)] {
            browser.query = TextArea::from([query]);
            browser.refilter();
            assert_eq!(browser.content_match_line(0), Some(line));
            browser.move_selection(0);
            assert_eq!(
                browser.preview_scroll,
                browser.records[0].preview_header_lines() + line
            );
        }
        let mut saved = snapshot();
        saved.records[0].set_body("changed first line\nchanged second line\nmarker".into());
        browser.replace(saved);
        assert_eq!(browser.content_match_line(0), Some(2));
        assert_eq!(
            browser.preview_scroll,
            browser.records[0].preview_header_lines() + 2
        );
        browser.search_mode = SearchMode::Ids;
        browser.refilter();
        assert_eq!(browser.content_match_line(0), None);
    }

    #[test]
    fn reference_selection_survives_filters_and_refresh() {
        let mut browser = Browser::new(ProjectScope::All);
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
