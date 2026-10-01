use std::path::PathBuf;

use anyhow::{Result, bail};
use crossterm::event::{KeyCode, KeyEvent};
use pwf_models::{
    note::NoteTitle,
    project::ProjectId,
    revision::ContentRevision,
    task::{
        BlockedBy, EffortTier, PriorityTier, TagInput, Task, TaskId, TaskReport, TaskTags,
        TaskTitle,
    },
};
use ratatui_textarea::TextArea;

use crate::text_input;

const FIELD_BYTES_MAX: usize = 4 * 1024 * 1024;
const TIERS: [&str; 5] = ["none", "low", "medium", "high", "highest"];

#[derive(Clone)]
pub(super) struct TaskTarget {
    pub id: TaskId,
    pub revision: ContentRevision,
    pub title: TaskTitle,
    pub tags: Option<TaskTags>,
    pub priority: Option<PriorityTier>,
    pub effort: Option<EffortTier>,
    pub blocked_by: Option<BlockedBy>,
}

impl From<Task> for TaskTarget {
    fn from(task: Task) -> Self {
        Self {
            id: task.id,
            revision: task.revision,
            title: task.title,
            tags: task.tags,
            priority: task.priority,
            effort: task.effort,
            blocked_by: task.blocked_by,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TaskAction {
    Metadata,
    Complete,
    CompleteReport,
    Cancel,
    Backlog,
    Activate,
    Delete,
    EditFile,
}

impl TaskAction {
    pub const ALL: [Self; 8] = [
        Self::Metadata,
        Self::EditFile,
        Self::Complete,
        Self::CompleteReport,
        Self::Cancel,
        Self::Backlog,
        Self::Activate,
        Self::Delete,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Metadata => "Edit title and metadata",
            Self::EditFile => "Edit Markdown in external editor",
            Self::Complete => "Complete quickly",
            Self::CompleteReport => "Complete with report and commits",
            Self::Cancel => "Cancel with report",
            Self::Backlog => "Move to backlog",
            Self::Activate => "Activate or reopen",
            Self::Delete => "Delete task",
        }
    }
}

pub(super) enum DraftKind {
    NewTask { project: ProjectId, preset: String },
    NewNote { project: ProjectId },
    Metadata(TaskTarget),
    Report { target: TaskTarget, cancel: bool },
    Export { references: String },
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FieldKind {
    Line,
    Body,
    Tier,
    Blockers,
}

pub(super) struct Field {
    pub label: &'static str,
    pub kind: FieldKind,
    pub input: TextArea<'static>,
}

impl Field {
    fn new(label: &'static str, kind: FieldKind, text: &str) -> Self {
        Self {
            label,
            kind,
            input: TextArea::from(text.split('\n')),
        }
    }

    pub fn text(&self) -> String {
        self.input.lines().join("\n")
    }

    pub fn set_text(&mut self, text: &str) {
        self.input = TextArea::from(text.split('\n'));
    }

    pub fn input_key(&mut self, key: KeyEvent) {
        if self.kind == FieldKind::Tier {
            let position = TIERS
                .iter()
                .position(|tier| *tier == self.text())
                .unwrap_or(0);
            let next = match key.code {
                KeyCode::Left => Some((position + TIERS.len() - 1) % TIERS.len()),
                KeyCode::Right | KeyCode::Enter | KeyCode::Char(' ') => {
                    Some((position + 1) % TIERS.len())
                }
                KeyCode::Backspace | KeyCode::Delete => Some(0),
                _ => None,
            };
            if let Some(next) = next {
                self.set_text(TIERS[next]);
            }
            return;
        }
        text_input::input_key(
            &mut self.input,
            key,
            self.kind == FieldKind::Body,
            FIELD_BYTES_MAX,
        );
    }

    fn bytes(&self) -> usize {
        text_input::bytes(&self.input)
    }

    pub fn paste(&mut self, text: &str) {
        if self.kind == FieldKind::Tier {
            return;
        }
        text_input::paste(
            &mut self.input,
            text,
            self.kind == FieldKind::Body,
            FIELD_BYTES_MAX,
        );
    }
}

pub(super) struct Draft {
    pub kind: DraftKind,
    pub fields: Vec<Field>,
    pub focused: usize,
    pub requires_inspection: bool,
    initial: Vec<String>,
}

pub(super) struct TaskEdit {
    pub target: TaskTarget,
    pub title: TaskTitle,
    pub tags: Option<TaskTags>,
    pub priority: Option<PriorityTier>,
    pub effort: Option<EffortTier>,
    pub blocked_by: Option<BlockedBy>,
}

pub(super) enum Mutation {
    CreateTask {
        project: ProjectId,
        title: TaskTitle,
        body: String,
    },
    CreateNote {
        project: ProjectId,
        title: NoteTitle,
        body: String,
    },
    UpdateTask(TaskEdit),
    Complete {
        target: TaskTarget,
        report: Option<TaskReport>,
        commits: Vec<String>,
    },
    Cancel {
        target: TaskTarget,
        report: TaskReport,
        commits: Vec<String>,
    },
    Backlog(TaskId),
    Activate(TaskId),
    Delete(TaskId),
    Export {
        path: PathBuf,
        references: String,
    },
}

impl Draft {
    fn new(kind: DraftKind, fields: Vec<Field>) -> Self {
        let initial = fields.iter().map(Field::text).collect();
        Self {
            kind,
            fields,
            focused: 0,
            requires_inspection: false,
            initial,
        }
    }

    pub fn new_task(project: ProjectId, preset: String, template: &str) -> Self {
        Self::new(
            DraftKind::NewTask { project, preset },
            vec![
                Field::new("Title", FieldKind::Line, ""),
                Field::new("Markdown body", FieldKind::Body, template),
            ],
        )
    }

    pub fn new_note(project: ProjectId) -> Self {
        Self::new(
            DraftKind::NewNote { project },
            vec![
                Field::new("Title", FieldKind::Line, ""),
                Field::new("Markdown body", FieldKind::Body, ""),
            ],
        )
    }

    pub fn metadata(target: TaskTarget) -> Self {
        let tags = target
            .tags
            .as_ref()
            .map(|tags| {
                tags.iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        let blockers = target
            .blocked_by
            .as_ref()
            .map(|ids| {
                ids.iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        let fields = vec![
            Field::new("Title", FieldKind::Line, target.title.as_ref()),
            Field::new(
                "Tags (comma separated; empty clears)",
                FieldKind::Line,
                &tags,
            ),
            Field::new(
                "Priority (left/right; none clears)",
                FieldKind::Tier,
                &target
                    .priority
                    .map_or_else(|| "none".to_string(), |tier| tier.to_string()),
            ),
            Field::new(
                "Effort (left/right; none clears)",
                FieldKind::Tier,
                &target
                    .effort
                    .map_or_else(|| "none".to_string(), |tier| tier.to_string()),
            ),
            Field::new(
                "Blocked by (Enter opens all projects; empty clears)",
                FieldKind::Blockers,
                &blockers,
            ),
        ];
        Self::new(DraftKind::Metadata(target), fields)
    }

    pub fn report(target: TaskTarget, cancel: bool) -> Self {
        Self::new(
            DraftKind::Report { target, cancel },
            vec![
                Field::new("Report", FieldKind::Body, ""),
                Field::new("Commits (optional; comma separated)", FieldKind::Line, ""),
            ],
        )
    }

    pub fn export(references: String) -> Self {
        Self::new(
            DraftKind::Export { references },
            vec![Field::new(
                "Export path (creates a new file)",
                FieldKind::Line,
                "",
            )],
        )
    }

    pub fn title(&self) -> String {
        match &self.kind {
            DraftKind::NewTask { project, preset } => format!("New task · {project} · {preset}"),
            DraftKind::NewNote { project } => format!("New note · {project}"),
            DraftKind::Metadata(target) => format!("Edit {}", target.id),
            DraftKind::Report { target, cancel } => format!(
                "{} {}",
                if *cancel { "Cancel" } else { "Complete" },
                target.id
            ),
            DraftKind::Export { .. } => "Export task references".to_string(),
        }
    }

    pub fn dirty(&self) -> bool {
        self.fields
            .iter()
            .zip(&self.initial)
            .any(|(field, initial)| field.text() != *initial)
    }

    pub fn focus_next(&mut self, previous: bool) {
        self.focused = if previous {
            (self.focused + self.fields.len() - 1) % self.fields.len()
        } else {
            (self.focused + 1) % self.fields.len()
        };
    }

    pub fn target(&self) -> Option<&TaskTarget> {
        match &self.kind {
            DraftKind::Metadata(target) | DraftKind::Report { target, .. } => Some(target),
            _ => None,
        }
    }

    pub fn inspected(&mut self, revision: Option<ContentRevision>) {
        if let Some(revision) = revision {
            match &mut self.kind {
                DraftKind::Metadata(target) | DraftKind::Report { target, .. } => {
                    target.revision = revision;
                }
                _ => {}
            }
        }
        self.requires_inspection = false;
    }

    pub fn mutation(&self) -> Result<Mutation> {
        if self
            .fields
            .iter()
            .any(|field| field.bytes() > FIELD_BYTES_MAX)
        {
            bail!("A draft field exceeds 4 MiB; shorten it before saving.");
        }
        if self.requires_inspection {
            bail!(
                "Press F5 to inspect saved state before retrying. The previous write may have succeeded."
            );
        }
        let text = |index: usize| self.fields[index].text();
        Ok(match &self.kind {
            DraftKind::NewTask { project, .. } => {
                if text(0).trim().is_empty() {
                    bail!("Enter a task title.");
                }
                Mutation::CreateTask {
                    project: project.clone(),
                    title: TaskTitle::try_new(text(0))?,
                    body: text(1),
                }
            }
            DraftKind::NewNote { project } => Mutation::CreateNote {
                project: project.clone(),
                title: NoteTitle::try_new(text(0))?,
                body: text(1),
            },
            DraftKind::Metadata(target) => Mutation::UpdateTask(TaskEdit {
                target: target.clone(),
                title: TaskTitle::try_new(text(0))?,
                tags: parse_tags(&text(1))?,
                priority: if text(2) == "none" {
                    None
                } else {
                    Some(text(2).parse()?)
                },
                effort: if text(3) == "none" {
                    None
                } else {
                    Some(text(3).parse()?)
                },
                blocked_by: parse_blockers(&text(4))?,
            }),
            DraftKind::Report { target, cancel } => {
                let report = TaskReport::try_new(text(0))?;
                let commits = text(1)
                    .split(',')
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(str::to_string)
                    .collect();
                if *cancel {
                    Mutation::Cancel {
                        target: target.clone(),
                        report,
                        commits,
                    }
                } else {
                    Mutation::Complete {
                        target: target.clone(),
                        report: Some(report),
                        commits,
                    }
                }
            }
            DraftKind::Export { references } => {
                if text(0).trim().is_empty() {
                    bail!("Enter an export path.");
                }
                Mutation::Export {
                    path: PathBuf::from(text(0).trim()),
                    references: references.clone(),
                }
            }
        })
    }
}

fn parse_tags(text: &str) -> Result<Option<TaskTags>> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let input: TagInput = text.parse()?;
    Ok(TaskTags::from_inputs(&[input]))
}

pub(super) fn parse_blockers(text: &str) -> Result<Option<BlockedBy>> {
    let ids = text
        .split(',')
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(|id| {
            id.strip_prefix("[[")
                .and_then(|id| id.strip_suffix("]]"))
                .unwrap_or(id)
                .parse()
        })
        .collect::<Result<Vec<TaskId>, _>>()?;
    if ids.is_empty() {
        Ok(None)
    } else {
        Ok(Some(BlockedBy::try_new(ids)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::task_target;

    #[test]
    fn metadata_form_parses_clears_and_canonical_cross_project_blockers() -> Result<()> {
        let mut draft = Draft::metadata(task_target());
        draft.fields[1].set_text("");
        draft.fields[2].set_text("none");
        draft.fields[3].set_text("highest");
        draft.fields[4].set_text("[[AUX-0003]], aux3, pwf8");
        let Mutation::UpdateTask(edit) = draft.mutation().unwrap() else {
            bail!("expected metadata edit");
        };
        assert!(edit.tags.is_none());
        assert!(edit.priority.is_none());
        assert_eq!(edit.effort, Some(EffortTier::Highest));
        assert_eq!(
            edit.blocked_by
                .unwrap()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            ["AUX-0003", "PWF-0008"]
        );
        assert_eq!(edit.target.revision.as_ref(), "a".repeat(64));
        Ok(())
    }

    #[test]
    fn failure_recovery_keeps_input_and_only_refreshes_revision() -> Result<()> {
        let mut draft = Draft::report(task_target(), false);
        draft.fields[0].set_text("Finished the keyboard journeys.");
        draft.fields[1].set_text("base..tip");
        draft.requires_inspection = true;
        assert!(draft.mutation().is_err());
        draft.inspected(Some(ContentRevision::try_new("b".repeat(64)).unwrap()));
        let Mutation::Complete {
            target,
            report,
            commits,
        } = draft.mutation().unwrap()
        else {
            bail!("expected completion");
        };
        assert_eq!(target.revision.as_ref(), "b".repeat(64));
        assert_eq!(report.unwrap().as_ref(), "Finished the keyboard journeys.");
        assert_eq!(commits, ["base..tip"]);
        Ok(())
    }

    #[test]
    fn single_line_paste_cannot_create_hidden_form_lines() {
        let mut field = Field::new("Title", FieldKind::Line, "");
        field.paste("first\nsecond");
        assert_eq!(field.text(), "first second");
    }
}
