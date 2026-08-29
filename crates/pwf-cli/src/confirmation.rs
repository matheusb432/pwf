//! Presents typed confirmation details and collects one terminal decision.

use std::fmt;

use anstyle::AnsiColor;
use dialoguer::{
    Confirm,
    console::Term,
    theme::{ColorfulTheme, Theme},
};
use pwf_client::task::{Confirmation, ConfirmationPrompt};

use crate::console::Console;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConfirmationAnswer {
    Accepted,
    Declined,
}

impl From<Option<bool>> for ConfirmationAnswer {
    fn from(answer: Option<bool>) -> Self {
        match answer {
            Some(true) => Self::Accepted,
            Some(false) | None => Self::Declined,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConfirmationDefault {
    Yes,
    No,
}

impl ConfirmationDefault {
    fn accepted(self) -> bool {
        matches!(self, Self::Yes)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConfirmationMode {
    AssumeYes,
    Prompt,
}

pub(crate) struct CliConfirmationClient {
    console: Console,
    mode: ConfirmationMode,
}

impl CliConfirmationClient {
    pub(crate) fn new(console: Console, mode: ConfirmationMode) -> Self {
        Self { console, mode }
    }
}

impl ConfirmationPrompt for CliConfirmationClient {
    type Error = dialoguer::Error;

    fn confirm(&self, confirmation: &Confirmation) -> Result<bool, Self::Error> {
        match self.mode {
            ConfirmationMode::AssumeYes => Ok(true),
            ConfirmationMode::Prompt => self
                .console
                .confirm(&confirmation_dialog(confirmation))
                .map(|answer| answer == ConfirmationAnswer::Accepted),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConfirmationTone {
    Destructive,
    Informational,
}

impl ConfirmationTone {
    fn heading_color(self) -> AnsiColor {
        match self {
            Self::Destructive => AnsiColor::Red,
            Self::Informational => AnsiColor::Cyan,
        }
    }
}

pub(crate) struct Detail {
    label: &'static str,
    value: String,
}

impl Detail {
    pub(crate) fn new(label: &'static str, value: impl fmt::Display) -> Self {
        let value = value.to_string();
        Self {
            label,
            value: flatten(&value),
        }
    }
}

pub(crate) struct ConfirmationDialog {
    heading: &'static str,
    details: Vec<Detail>,
    question: &'static str,
    default: ConfirmationDefault,
    tone: ConfirmationTone,
}

impl ConfirmationDialog {
    pub(crate) fn new(
        heading: &'static str,
        details: Vec<Detail>,
        question: &'static str,
        default: ConfirmationDefault,
        tone: ConfirmationTone,
    ) -> Self {
        Self {
            heading,
            details,
            question,
            default,
            tone,
        }
    }

    pub(crate) fn render(&self, color: bool) -> String {
        let label_width = self
            .details
            .iter()
            .map(|detail| detail.label.len())
            .max()
            .unwrap_or_default();
        let mut lines = vec![
            paint(self.heading, self.tone.heading_color(), color),
            String::new(),
        ];
        lines.extend(self.details.iter().map(|detail| {
            let label = format!("{:<label_width$}", detail.label);
            format!(
                "  {}  {}",
                paint(&label, AnsiColor::Cyan, color),
                detail.value
            )
        }));
        lines.join("\n")
    }

    pub(crate) fn interact(&self, color: bool) -> dialoguer::Result<ConfirmationAnswer> {
        let terminal = Term::stderr();
        terminal.write_line(&self.render(color))?;
        terminal.write_line("")?;
        let theme = ConfirmationTheme::default();
        Confirm::with_theme(&theme)
            .with_prompt(self.question)
            .default(self.default.accepted())
            .interact_on_opt(&terminal)
            .map(ConfirmationAnswer::from)
    }
}

pub(crate) fn prompt_error(context: &str, source: dialoguer::Error) -> anyhow::Error {
    let detail = source.to_string();
    anyhow::Error::new(source).context(format!("{context} confirmation failed: {detail}"))
}

fn confirmation_dialog(confirmation: &Confirmation) -> ConfirmationDialog {
    match confirmation {
        Confirmation::RemoveTask(confirmation) => ConfirmationDialog::new(
            "Confirm task removal",
            vec![
                Detail::new("Task", &confirmation.task_id),
                Detail::new("Title", &confirmation.title),
                Detail::new("Status", task_status(confirmation.status)),
                Detail::new("Project", &confirmation.project),
                Detail::new("Note", &confirmation.note_path),
            ],
            "Remove this task?",
            ConfirmationDefault::No,
            ConfirmationTone::Destructive,
        ),
        Confirmation::ReopenTask(confirmation) => ConfirmationDialog::new(
            "Reopen task and delete completion data",
            vec![
                Detail::new("Task", &confirmation.task_id),
                Detail::new("Project", &confirmation.project),
                Detail::new(
                    "Completed",
                    optional_text(confirmation.completion_date.as_deref()),
                ),
                Detail::new("Commits", optional_text(confirmation.commits.as_deref())),
                Detail::new("Report", optional_text(confirmation.report.as_deref())),
            ],
            "Delete this completion data and reopen the task?",
            ConfirmationDefault::No,
            ConfirmationTone::Destructive,
        ),
        Confirmation::DispatchSession(preflight) => {
            let confirmation = preflight.confirmation.as_ref();
            let identity = confirmation.map_or("(unknown)", |value| value.session_name.as_str());
            let is_compound = confirmation.is_some_and(|value| value.task_ids.len() > 1);
            let task_label = if is_compound { "Tasks" } else { "Task" };
            let mut details = vec![Detail::new(task_label, identity)];
            if !is_compound {
                details.push(Detail::new(
                    "Title",
                    confirmation.map_or("(unknown)", |value| value.title.as_str()),
                ));
            }
            ConfirmationDialog::new(
                "Confirm session dispatch",
                details,
                "Proceed with session dispatch?",
                ConfirmationDefault::Yes,
                ConfirmationTone::Informational,
            )
        }
    }
}

fn task_status(value: i32) -> &'static str {
    match pwf_client::v1::TaskStatus::try_from(value).ok() {
        Some(pwf_client::v1::TaskStatus::Active) => "active",
        Some(pwf_client::v1::TaskStatus::Done) => "done",
        Some(pwf_client::v1::TaskStatus::Cancelled) => "cancelled",
        Some(pwf_client::v1::TaskStatus::Unspecified) | None => "unspecified",
    }
}

fn optional_text(value: Option<&str>) -> &str {
    match value {
        Some("") => "(empty)",
        Some(value) => value,
        None => "(not recorded)",
    }
}

fn flatten(value: &str) -> String {
    value.replace("\r\n", " ").replace(['\r', '\n'], " ")
}

fn paint(text: &str, color: AnsiColor, enabled: bool) -> String {
    if !enabled {
        return text.to_string();
    }
    let style = anstyle::Style::new().bold().fg_color(Some(color.into()));
    format!("{}{}{}", style.render(), text, style.render_reset())
}

#[derive(Default)]
struct ConfirmationTheme(ColorfulTheme);

impl Theme for ConfirmationTheme {
    fn format_confirm_prompt(
        &self,
        formatter: &mut dyn fmt::Write,
        prompt: &str,
        default: Option<bool>,
    ) -> fmt::Result {
        self.0.format_confirm_prompt(formatter, prompt, default)
    }

    fn format_confirm_prompt_selection(
        &self,
        formatter: &mut dyn fmt::Write,
        prompt: &str,
        selection: Option<bool>,
    ) -> fmt::Result {
        if selection.is_some() {
            return self
                .0
                .format_confirm_prompt_selection(formatter, prompt, selection);
        }
        write!(
            formatter,
            "{} {} {} {}",
            self.0.error_prefix,
            self.0.prompt_style.apply_to(prompt),
            self.0.success_suffix,
            self.0.error_style.apply_to("cancelled")
        )
    }
}

#[cfg(test)]
mod tests {
    use pwf_client::v1::{RemoveTaskConfirmation, ReopenTaskConfirmation, TaskStatus};

    use super::*;

    #[test]
    fn renders_aligned_plain_details_and_flattens_line_endings() {
        let dialog = ConfirmationDialog::new(
            "Confirm",
            vec![
                Detail::new("Task", "FOO-0001"),
                Detail::new("Long label", "first\nsecond\r\nthird"),
            ],
            "Proceed?",
            ConfirmationDefault::Yes,
            ConfirmationTone::Informational,
        );

        assert_eq!(
            dialog.render(false),
            "Confirm\n\n  Task        FOO-0001\n  Long label  first second third"
        );
    }

    #[test]
    fn cancellation_is_a_declined_answer() {
        assert_eq!(ConfirmationAnswer::from(None), ConfirmationAnswer::Declined);
    }

    #[test]
    fn removal_confirmation_renders_aligned_task_details() {
        let confirmation = Confirmation::RemoveTask(RemoveTaskConfirmation {
            task_id: "FOO-0001".to_string(),
            project: "foo".to_string(),
            title: "stale task".to_string(),
            status: TaskStatus::Active as i32,
            note_path: "/notes/foo/FOO-0001.md".to_string(),
        });

        assert_eq!(
            confirmation_dialog(&confirmation).render(false),
            "Confirm task removal\n\n  Task     FOO-0001\n  Title    stale task\n  Status   active\n  Project  foo\n  Note     /notes/foo/FOO-0001.md"
        );
    }

    #[test]
    fn reopen_confirmation_emphasizes_every_deleted_artifact() {
        let confirmation = Confirmation::ReopenTask(ReopenTaskConfirmation {
            task_id: "FOO-0001".to_string(),
            project: "foo".to_string(),
            completion_date: Some("2026-08-17".to_string()),
            commits: Some("abc..def".to_string()),
            report: Some("validated the release".to_string()),
        });

        assert_eq!(
            confirmation_dialog(&confirmation).render(false),
            "Reopen task and delete completion data\n\n  Task       FOO-0001\n  Project    foo\n  Completed  2026-08-17\n  Commits    abc..def\n  Report     validated the release"
        );
    }
}
