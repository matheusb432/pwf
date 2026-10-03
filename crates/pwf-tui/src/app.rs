use std::{collections::BTreeSet, path::PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use pwf_models::{project::ProjectId, revision::ContentRevision, task::TaskId};
use ratatui_textarea::TextArea;

use crate::{
    backend::{Blocker, Outcome, Question, Work},
    browser::{Browser, ProjectScope, SearchMode},
    draft::{Draft, DraftKind, FieldKind, TaskAction, parse_blockers},
    text_input,
};

const QUERY_BYTES_MAX: usize = 4096;

pub(super) enum Effect {
    Work(Work),
    CancelRead,
    Confirm(bool),
    EditFile(PathBuf),
    EditField(String),
    Copy(String),
    Quit,
}

#[derive(PartialEq, Eq)]
pub(super) enum NoticeKind {
    Loading,
    Info,
    Error,
}

pub(super) struct Notice {
    pub text: String,
    pub kind: NoticeKind,
}

impl Notice {
    pub fn is_error(&self) -> bool {
        self.kind == NoticeKind::Error
    }
}

#[derive(Clone, Copy)]
pub(super) enum ProjectPurpose {
    Browse,
    NewTask,
    NewNote,
}

pub(super) struct BlockerPicker {
    pub candidates: Vec<Blocker>,
    pub query: TextArea<'static>,
    pub selected: usize,
    pub marked: BTreeSet<TaskId>,
}

impl BlockerPicker {
    fn handle_key(&mut self, key: KeyEvent) -> Option<String> {
        let visible = self.visible();
        match key.code {
            KeyCode::Enter => {
                return Some(
                    self.marked
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(", "),
                );
            }
            KeyCode::Up | KeyCode::Down | KeyCode::Home | KeyCode::End => {
                move_position(&mut self.selected, visible.len(), key.code);
            }
            KeyCode::Char(' ') => {
                let index = *visible.get(self.selected)?;
                let id = self.candidates[index].id.clone();
                if !self.marked.remove(&id) {
                    self.marked.insert(id);
                }
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.marked.clear();
            }
            _ => {
                text_input::input_key(&mut self.query, key, false, QUERY_BYTES_MAX);
                self.selected = 0;
            }
        }
        None
    }

    pub fn visible(&self) -> Vec<usize> {
        let query = self.query.lines().join(" ").to_lowercase();
        self.candidates
            .iter()
            .enumerate()
            .filter(|(_, candidate)| {
                format!("{} {}", candidate.id, candidate.title)
                    .to_lowercase()
                    .contains(&query)
            })
            .map(|(index, _)| index)
            .collect()
    }
}

pub(super) enum Dialog {
    Projects {
        purpose: ProjectPurpose,
        selected: usize,
    },
    Actions {
        id: TaskId,
        selected: usize,
    },
    Blockers(Box<BlockerPicker>),
    Help {
        scroll: usize,
    },
    Inspection {
        revision: Option<ContentRevision>,
        current: String,
        scroll: usize,
    },
}

#[derive(Clone, Copy)]
pub(super) enum LocalQuestion {
    DiscardDraft,
    QuitDraft,
}

pub(super) struct ConfirmationView {
    pub question: Question,
    pub affirmative: bool,
    pub local: Option<LocalQuestion>,
    pub scroll: u16,
}

pub(super) struct Pending {
    pub id: u64,
    pub writes: bool,
}

pub(super) struct App {
    pub browser: Browser,
    pub draft: Option<Draft>,
    pub dialog: Option<Dialog>,
    pub confirmation: Option<ConfirmationView>,
    pub pending: Option<Pending>,
    pub notice: Notice,
    pub tick: usize,
}

impl App {
    pub fn new(project_scope: ProjectScope) -> Self {
        Self {
            browser: Browser::new(project_scope),
            draft: None,
            dialog: None,
            confirmation: None,
            pending: None,
            notice: Notice {
                text: "Loading saved tasks and notes…".into(),
                kind: NoticeKind::Loading,
            },
            tick: 0,
        }
    }

    pub fn load(&self) -> Effect {
        Effect::Work(Work::Load {
            scope: self.browser.project_scope.clone(),
        })
    }

    pub fn notify(&mut self, text: impl Into<String>, error: bool) {
        self.notice = Notice {
            text: text.into(),
            kind: if error {
                NoticeKind::Error
            } else {
                NoticeKind::Info
            },
        };
    }

    fn loading(&mut self) {
        self.notice = Notice {
            text: "Loading saved tasks and notes…".into(),
            kind: NoticeKind::Loading,
        };
    }

    pub fn ask_server(&mut self, question: Question) {
        self.confirmation = Some(ConfirmationView {
            question,
            affirmative: false,
            local: None,
            scroll: 0,
        });
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Option<Effect> {
        if key.kind == KeyEventKind::Release {
            return None;
        }
        if self.confirmation.is_some() {
            return self.handle_confirmation(key);
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return self.quit();
        }
        if self.dialog.is_some() {
            return self.handle_dialog(key);
        }
        if self.draft.is_some() {
            return self.handle_draft(key);
        }
        if self.browser.searching {
            return self.handle_search(key);
        }
        self.handle_browser(key)
    }

    pub fn paste(&mut self, text: &str) {
        if self.pending.is_some()
            || self.confirmation.is_some()
            || self
                .dialog
                .as_ref()
                .is_some_and(|dialog| !matches!(dialog, Dialog::Blockers(_)))
        {
            return;
        }
        if let Some(Dialog::Blockers(picker)) = &mut self.dialog {
            text_input::paste(&mut picker.query, text, false, QUERY_BYTES_MAX);
            picker.selected = 0;
        } else if let Some(draft) = &mut self.draft {
            draft.fields[draft.focused].paste(text);
        } else if self.browser.searching {
            text_input::paste(&mut self.browser.query, text, false, QUERY_BYTES_MAX);
            self.browser.selected = 0;
            self.browser.refilter();
        }
    }

    fn handle_browser(&mut self, key: KeyEvent) -> Option<Effect> {
        match key.code {
            KeyCode::Char('q') => return self.quit(),
            KeyCode::Char('?') => self.dialog = Some(Dialog::Help { scroll: 0 }),
            KeyCode::Up | KeyCode::Char('k') => self.browser.move_selection(-1),
            KeyCode::Down | KeyCode::Char('j') => self.browser.move_selection(1),
            KeyCode::Home => self.browser.move_selection(isize::MIN),
            KeyCode::End => self.browser.move_selection(isize::MAX),
            KeyCode::PageUp => self.browser.scroll_preview(-10),
            KeyCode::PageDown => self.browser.scroll_preview(10),
            KeyCode::Char('/') => self.browser.searching = true,
            KeyCode::Char('f') => self.toggle_search_mode(),
            KeyCode::Char('s') => {
                self.browser.status = self.browser.status.next();
                self.browser.refilter();
            }
            KeyCode::Tab => {
                self.browser.kind = self.browser.kind.next();
                self.browser.refilter();
            }
            KeyCode::Char(' ') => self.browser.toggle_reference(),
            KeyCode::Char('u') => self.browser.references.clear(),
            KeyCode::Char('p') => {
                self.dialog = Some(Dialog::Projects {
                    purpose: ProjectPurpose::Browse,
                    selected: 0,
                });
            }
            KeyCode::Char('r') | KeyCode::F(5) if self.pending.is_none() => {
                self.loading();
                return Some(self.load());
            }
            KeyCode::Esc if self.pending.as_ref().is_some_and(|pending| !pending.writes) => {
                return Some(Effect::CancelRead);
            }
            KeyCode::Esc => {
                self.browser.query = TextArea::default();
                self.browser.refilter();
            }
            KeyCode::Char('t') if self.pending.is_none() => {
                return self.create(ProjectPurpose::NewTask);
            }
            KeyCode::Char('n') if self.pending.is_none() => {
                return self.create(ProjectPurpose::NewNote);
            }
            KeyCode::Char('y') => {
                if let Some(text) = self.browser.reference_text() {
                    return Some(Effect::Copy(text));
                }
                self.notify("Select a task to copy its reference.", true);
            }
            KeyCode::Char('x') => {
                if let Some(text) = self.browser.reference_text() {
                    self.draft = Some(Draft::export(text));
                } else {
                    self.notify("Select a task to export its reference.", true);
                }
            }
            KeyCode::Enter if self.pending.is_none() => return self.open_record(),
            code if self.pending.is_none() => {
                return self.action(match code {
                    KeyCode::Char('e') => TaskAction::EditFile,
                    KeyCode::Char('m') => TaskAction::Metadata,
                    KeyCode::Char('d') => TaskAction::Complete,
                    KeyCode::Char('D') => TaskAction::CompleteReport,
                    KeyCode::Char('c') => TaskAction::Cancel,
                    KeyCode::Char('b') => TaskAction::Backlog,
                    KeyCode::Char('a') => TaskAction::Activate,
                    KeyCode::Delete => TaskAction::Delete,
                    _ => return None,
                });
            }
            _ => {}
        }
        None
    }

    fn action(&mut self, action: TaskAction) -> Option<Effect> {
        let record = self.browser.current()?;
        if let Some(id) = record.id.task() {
            return Some(Effect::Work(Work::Action {
                id: id.clone(),
                action,
            }));
        }
        if action == TaskAction::EditFile {
            return Some(Effect::EditFile(record.path.clone()));
        }
        self.notify(
            "This action needs a task. Notes open in the external editor with e.",
            true,
        );
        None
    }

    fn toggle_search_mode(&mut self) {
        self.browser.search_mode = match self.browser.search_mode {
            SearchMode::Ids => SearchMode::Contents,
            SearchMode::Contents => SearchMode::Ids,
        };
        self.browser.refilter();
    }

    fn handle_search(&mut self, key: KeyEvent) -> Option<Effect> {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => self.browser.searching = false,
            KeyCode::Down => self.browser.move_selection(1),
            KeyCode::Up => self.browser.move_selection(-1),
            KeyCode::Char('g') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.toggle_search_mode();
            }
            _ => {
                if text_input::input_key(&mut self.browser.query, key, false, QUERY_BYTES_MAX) {
                    self.browser.selected = 0;
                    self.browser.refilter();
                }
            }
        }
        None
    }

    fn create(&mut self, purpose: ProjectPurpose) -> Option<Effect> {
        if let Some(project) = self.browser.project_scope.project()
            && self
                .browser
                .projects
                .iter()
                .any(|candidate| &candidate.id == project)
        {
            return self.create_for_project(purpose, project.clone());
        }
        self.dialog = Some(Dialog::Projects {
            purpose,
            selected: 0,
        });
        None
    }

    fn create_for_project(
        &mut self,
        purpose: ProjectPurpose,
        project: ProjectId,
    ) -> Option<Effect> {
        match purpose {
            ProjectPurpose::NewTask => Some(Effect::Work(Work::NewTask(project))),
            ProjectPurpose::NewNote => {
                self.draft = Some(Draft::new_note(project));
                None
            }
            ProjectPurpose::Browse => None,
        }
    }

    fn handle_dialog(&mut self, key: KeyEvent) -> Option<Effect> {
        if key.code == KeyCode::Esc {
            self.dialog = None;
            return None;
        }
        match self.dialog.take()? {
            Dialog::Help { mut scroll } => {
                if !matches!(key.code, KeyCode::Char('?') | KeyCode::Enter) {
                    scroll_text(&mut scroll, usize::MAX, key.code);
                    self.dialog = Some(Dialog::Help { scroll });
                }
                None
            }
            Dialog::Inspection {
                revision,
                current,
                mut scroll,
            } => {
                if key.code != KeyCode::Enter {
                    scroll_text(&mut scroll, current.lines().count(), key.code);
                    self.dialog = Some(Dialog::Inspection {
                        revision,
                        current,
                        scroll,
                    });
                    return None;
                }
                if let Some(draft) = &mut self.draft {
                    draft.inspected(revision);
                }
                self.notify("Saved state inspected. Draft retained; Ctrl-s applies edited fields to this revision.", false);
                None
            }
            Dialog::Projects { purpose, selected } => self.handle_projects(key, purpose, selected),
            Dialog::Actions { id, mut selected } => {
                move_position(&mut selected, TaskAction::ALL.len(), key.code);
                if key.code == KeyCode::Enter && self.pending.is_none() {
                    return Some(Effect::Work(Work::Action {
                        id,
                        action: TaskAction::ALL[selected],
                    }));
                }
                self.dialog = Some(Dialog::Actions { id, selected });
                None
            }
            Dialog::Blockers(mut picker) => {
                if let Some(text) = picker.handle_key(key) {
                    let draft = self.draft.as_mut()?;
                    draft.fields[draft.focused].set_text(&text);
                } else {
                    self.dialog = Some(Dialog::Blockers(picker));
                }
                None
            }
        }
    }

    fn open_record(&mut self) -> Option<Effect> {
        let record = self.browser.current()?;
        let Some(id) = record.id.task() else {
            return Some(Effect::EditFile(record.path.clone()));
        };
        self.dialog = Some(Dialog::Actions {
            id: id.clone(),
            selected: 0,
        });
        None
    }

    fn handle_projects(
        &mut self,
        key: KeyEvent,
        purpose: ProjectPurpose,
        mut selected: usize,
    ) -> Option<Effect> {
        let browsing = matches!(purpose, ProjectPurpose::Browse);
        let count = self.browser.projects.len() + usize::from(browsing);
        move_position(&mut selected, count, key.code);
        if key.code != KeyCode::Enter || self.pending.is_some() {
            self.dialog = Some(Dialog::Projects { purpose, selected });
            return None;
        }
        if browsing {
            self.browser.project_scope = selected
                .checked_sub(1)
                .and_then(|index| self.browser.projects.get(index))
                .map(|project| project.id.clone())
                .into();
            self.browser.refilter();
            self.loading();
            return Some(self.load());
        }
        let project = self.browser.projects.get(selected)?.id.clone();
        self.create_for_project(purpose, project)
    }

    fn handle_draft(&mut self, key: KeyEvent) -> Option<Effect> {
        if self.pending.is_some() {
            if key.code == KeyCode::Esc
                && self.pending.as_ref().is_some_and(|pending| !pending.writes)
            {
                return Some(Effect::CancelRead);
            }
            self.notify("Waiting for the server; your draft is retained.", false);
            return None;
        }
        if key.code == KeyCode::Esc {
            if self.draft.as_ref().is_some_and(Draft::dirty) {
                self.ask_local(LocalQuestion::DiscardDraft);
            } else {
                self.draft = None;
            }
            return None;
        }
        let draft = self.draft.as_mut()?;
        match key.code {
            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                match draft.mutation() {
                    Ok(mutation) => return Some(Effect::Work(Work::Submit(mutation))),
                    Err(error) => self.notify(error.to_string(), true),
                }
            }
            KeyCode::F(5) if draft.requires_inspection => {
                return Some(Effect::Work(Work::Inspect {
                    project: self.browser.project_scope.project().cloned(),
                    task: draft.target().map(|target| target.id.clone()),
                }));
            }
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if draft.fields[draft.focused].kind != FieldKind::Tier {
                    return Some(Effect::EditField(draft.fields[draft.focused].text()));
                }
            }
            KeyCode::Tab => draft.focus_next(false),
            KeyCode::BackTab => draft.focus_next(true),
            KeyCode::Enter if draft.fields[draft.focused].kind == FieldKind::Blockers => {
                if let Err(error) = parse_blockers(&draft.fields[draft.focused].text()) {
                    self.notify(error.to_string(), true);
                    return None;
                }
                if let Some(target) = draft.target() {
                    return Some(Effect::Work(Work::Blockers {
                        exclude: target.id.clone(),
                    }));
                }
            }
            _ => draft.fields[draft.focused].input_key(key),
        }
        None
    }

    fn quit(&mut self) -> Option<Effect> {
        if self.pending.as_ref().is_some_and(|pending| pending.writes) {
            self.notify(
                "A submitted write is still running. Wait for its result before quitting.",
                true,
            );
            return None;
        }
        if self.draft.as_ref().is_some_and(Draft::dirty) {
            self.ask_local(LocalQuestion::QuitDraft);
            return None;
        }
        Some(Effect::Quit)
    }

    fn ask_local(&mut self, local: LocalQuestion) {
        self.confirmation = Some(ConfirmationView {
            question: Question {
                title: "Discard this draft?".into(),
                lines: vec!["Your unsaved input will be discarded.".into()],
            },
            affirmative: false,
            local: Some(local),
            scroll: 0,
        });
    }

    fn handle_confirmation(&mut self, key: KeyEvent) -> Option<Effect> {
        let confirmation = self.confirmation.as_mut()?;
        match key.code {
            KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab => {
                confirmation.affirmative = !confirmation.affirmative;
            }
            KeyCode::Char('y') => confirmation.affirmative = true,
            KeyCode::Char('n') => confirmation.affirmative = false,
            KeyCode::Up | KeyCode::PageUp => {
                confirmation.scroll = confirmation.scroll.saturating_sub(5);
            }
            KeyCode::Down | KeyCode::PageDown => {
                confirmation.scroll = confirmation.scroll.saturating_add(5);
            }
            KeyCode::Enter | KeyCode::Esc => {
                let confirmed = key.code == KeyCode::Enter && confirmation.affirmative;
                let local = confirmation.local;
                self.confirmation = None;
                let Some(local) = local else {
                    return Some(Effect::Confirm(confirmed));
                };
                if !confirmed {
                    return None;
                }
                self.draft = None;
                if matches!(local, LocalQuestion::QuitDraft) {
                    return Some(Effect::Quit);
                }
            }
            _ => {}
        }
        None
    }

    pub fn finished(&mut self, id: u64, result: Result<Outcome, String>) -> Option<Effect> {
        if self.pending.as_ref().is_none_or(|pending| pending.id != id) {
            return None;
        }
        let writes = self.pending.take().is_some_and(|pending| pending.writes);
        self.confirmation = None;
        match result {
            Err(message) => {
                let recovery = match (writes, self.draft.as_mut()) {
                    (true, Some(draft)) if matches!(draft.kind, DraftKind::Export { .. }) => {
                        draft.requires_inspection = false;
                        "Draft retained. Choose a new path and retry. "
                    }
                    (true, Some(draft)) => {
                        draft.requires_inspection = true;
                        "Draft retained. F5 inspects saved state before retrying. "
                    }
                    (true, None) => "Inspect saved state with r before retrying. ",
                    (false, Some(draft)) if draft.requires_inspection => {
                        "Draft retained. F5 inspects saved state again; Esc returns. "
                    }
                    (false, Some(_)) => {
                        "Draft retained. Enter retries blocker selection; Esc returns. "
                    }
                    (false, None) => "Press r to refresh or Esc to return. ",
                };
                self.notify(format!("{recovery}{message}"), true);
            }
            Ok(Outcome::Loaded(snapshot)) => {
                let count = snapshot.records.len();
                let warning = snapshot.warnings.first().cloned();
                self.browser.replace(snapshot);
                if let Some(warning) = warning {
                    self.notify(warning, true);
                } else if self.notice.kind == NoticeKind::Loading {
                    self.notify(format!("Loaded {count} saved records. / searches; Enter opens actions; ? shows keys."), false);
                }
            }
            Ok(Outcome::Draft(draft)) => {
                self.draft = Some(draft);
                self.notify("Ctrl-s saves · Tab changes fields · Ctrl-e opens the focused field in your editor.", false);
            }
            Ok(Outcome::Blockers(candidates)) => {
                let marked = self
                    .draft
                    .as_ref()
                    .and_then(|draft| {
                        parse_blockers(&draft.fields[draft.focused].text())
                            .ok()
                            .flatten()
                    })
                    .map(|ids| ids.into_iter().collect())
                    .unwrap_or_default();
                self.dialog = Some(Dialog::Blockers(Box::new(BlockerPicker {
                    candidates,
                    query: TextArea::default(),
                    selected: 0,
                    marked,
                })));
            }
            Ok(Outcome::Saved { message }) => {
                self.draft = None;
                self.notify(message, false);
                return Some(self.load());
            }
            Ok(Outcome::Exported(path)) => {
                self.draft = None;
                self.notify(format!("Exported references to {}", path.display()), false);
            }
            Ok(Outcome::Aborted) => self.notify("Cancelled; no approval was sent.", false),
            Ok(Outcome::EditFile(path)) => return Some(Effect::EditFile(path)),
            Ok(Outcome::Inspected {
                snapshot,
                revision,
                current,
            }) => {
                self.browser.replace(snapshot);
                self.dialog = Some(Dialog::Inspection {
                    revision,
                    current,
                    scroll: 0,
                });
                self.notify(
                    "Inspect saved state. Enter enables another save; Esc keeps the draft locked.",
                    false,
                );
            }
        }
        None
    }

    pub fn edited_field(&mut self, text: &str, error: Option<String>) {
        if let Some(draft) = &mut self.draft {
            let field = &mut draft.fields[draft.focused];
            let text = if field.kind == FieldKind::Body {
                text.to_string()
            } else {
                text.replace(['\r', '\n'], " ")
            };
            field.set_text(&text);
        }
        match error {
            Some(error) => {
                self.notify(format!("{error} Edited input retained in the draft."), true);
            }
            None => self.notify("Editor input retained. Ctrl-s saves the draft.", false),
        }
    }
}

fn scroll_text(scroll: &mut usize, lines: usize, key: KeyCode) {
    *scroll = match key {
        KeyCode::Up | KeyCode::Char('k') => scroll.saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') => scroll.saturating_add(1),
        KeyCode::PageUp => scroll.saturating_sub(10),
        KeyCode::PageDown => scroll.saturating_add(10),
        KeyCode::Home => 0,
        KeyCode::End => lines.saturating_sub(1),
        _ => *scroll,
    }
    .min(lines.saturating_sub(1));
}

fn move_position(position: &mut usize, count: usize, key: KeyCode) {
    *position = match key {
        KeyCode::Up | KeyCode::Char('k') => position.saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') => {
            position.saturating_add(1).min(count.saturating_sub(1))
        }
        KeyCode::Home => 0,
        KeyCode::End => count.saturating_sub(1),
        _ => *position,
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{app, snapshot};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn keyboard_journey_searches_contents_marks_references_and_opens_actions() {
        let mut app = app();
        app.handle_key(key(KeyCode::Char('/')));
        app.paste("saved markdown");
        assert_eq!(app.browser.visible, Vec::<usize>::new());
        app.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL));
        assert_eq!(app.browser.visible.len(), 2);
        app.handle_key(key(KeyCode::Enter));
        app.handle_key(key(KeyCode::Char(' ')));
        assert!(
            matches!(app.handle_key(key(KeyCode::Char('y'))), Some(Effect::Copy(text)) if text == "[[PWF-0007]]")
        );
        app.handle_key(key(KeyCode::Char('x')));
        app.paste("references.md");
        assert!(matches!(
            app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)),
            Some(Effect::Work(Work::Submit(
                crate::draft::Mutation::Export { .. }
            )))
        ));
        app.pending = Some(Pending {
            id: 1,
            writes: true,
        });
        assert!(
            app.finished(1, Ok(Outcome::Exported("references.md".into())))
                .is_none()
        );
        assert_eq!(
            app.browser.reference_text().as_deref(),
            Some("[[PWF-0007]]")
        );
        app.handle_key(key(KeyCode::Enter));
        assert!(matches!(app.dialog, Some(Dialog::Actions { .. })));
        assert!(matches!(
            app.handle_key(key(KeyCode::Enter)),
            Some(Effect::Work(Work::Action {
                action: TaskAction::Metadata,
                ..
            }))
        ));
    }

    #[test]
    fn search_cursor_moves_keep_selection_and_preview_until_the_query_changes() {
        let mut app = app();
        app.handle_key(key(KeyCode::Char('f')));
        app.handle_key(key(KeyCode::Char('/')));
        app.paste("saved");
        app.handle_key(key(KeyCode::Down));
        let scroll = app.browser.preview_scroll;
        for input in [
            key(KeyCode::Home),
            key(KeyCode::End),
            key(KeyCode::Left),
            key(KeyCode::Right),
            key(KeyCode::Delete),
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL),
            KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL),
            key(KeyCode::F(12)),
        ] {
            app.handle_key(input);
            assert_eq!(app.browser.query.lines(), ["saved"]);
            assert_eq!(app.browser.selected, 1, "{input:?}");
            assert_eq!(app.browser.preview_scroll, scroll, "{input:?}");
        }
        app.handle_key(key(KeyCode::Char('x')));
        assert_eq!(app.browser.visible, Vec::<usize>::new());
        app.handle_key(key(KeyCode::Backspace));
        assert_eq!(app.browser.visible.len(), 2);
        assert_eq!(app.browser.selected, 0);
    }

    #[test]
    fn discard_and_server_confirmations_default_to_no() {
        let mut app = app();
        app.draft = Some(Draft::new_note("pwf".parse().unwrap()));
        app.paste("A retained draft");
        app.handle_key(key(KeyCode::Esc));
        assert!(!app.confirmation.as_ref().unwrap().affirmative);
        app.handle_key(key(KeyCode::Enter));
        assert_eq!(
            app.draft.as_ref().unwrap().fields[0].text(),
            "A retained draft"
        );
        app.ask_server(Question {
            title: "Delete?".into(),
            lines: vec!["Hard delete".into()],
        });
        assert!(matches!(
            app.handle_key(key(KeyCode::Enter)),
            Some(Effect::Confirm(false))
        ));
        app.ask_server(Question {
            title: "Delete?".into(),
            lines: Vec::new(),
        });
        app.handle_key(key(KeyCode::Char('y')));
        assert!(matches!(
            app.handle_key(key(KeyCode::Esc)),
            Some(Effect::Confirm(false))
        ));
    }

    #[test]
    fn failed_creation_keeps_draft_and_requires_explicit_inspection() {
        let mut app = app();
        app.draft = Some(Draft::new_task(
            "pwf".parse().unwrap(),
            "default".into(),
            "## Goals\n\n",
        ));
        app.paste("Keep my title");
        app.pending = Some(Pending {
            id: 1,
            writes: true,
        });
        assert!(app.finished(1, Err("connection lost".into())).is_none());
        assert_eq!(
            app.draft.as_ref().unwrap().fields[0].text(),
            "Keep my title"
        );
        assert!(
            app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL))
                .is_none()
        );
        assert!(matches!(
            app.handle_key(key(KeyCode::F(5))),
            Some(Effect::Work(Work::Inspect { task: None, .. }))
        ));
        for (id, code, requires_inspection) in [(2, KeyCode::Esc, true), (3, KeyCode::Enter, false)]
        {
            app.pending = Some(Pending { id, writes: false });
            app.finished(
                id,
                Ok(Outcome::Inspected {
                    snapshot: snapshot(),
                    revision: None,
                    current: "Saved records".into(),
                }),
            );
            app.handle_key(key(code));
            let draft = app.draft.as_ref().unwrap();
            assert_eq!(draft.requires_inspection, requires_inspection);
            assert_eq!(draft.fields[0].text(), "Keep my title");
        }
    }

    #[test]
    fn cancelled_reads_ignore_late_results_and_submitted_writes_prevent_quit() {
        let mut app = app();
        app.pending = Some(Pending {
            id: 2,
            writes: false,
        });
        assert!(matches!(
            app.handle_key(key(KeyCode::Esc)),
            Some(Effect::CancelRead)
        ));
        app.pending = None;
        app.finished(
            2,
            Ok(Outcome::Draft(Draft::new_note("pwf".parse().unwrap()))),
        );
        assert!(app.draft.is_none());
        app.pending = Some(Pending {
            id: 3,
            writes: true,
        });
        assert!(app.handle_key(key(KeyCode::Char('q'))).is_none());
        assert!(app.notice.is_error());
    }
}
