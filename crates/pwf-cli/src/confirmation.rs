//! Presents typed confirmation details and collects one terminal decision.

use std::{fmt, sync::OnceLock};

use anstyle::AnsiColor;
use dialoguer::{
    Confirm,
    console::Term,
    theme::{ColorfulTheme, Theme},
};
use pwf_application::ports::confirmation::ConfirmationClient;
use pwf_wire::confirmation::Confirmation;

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
    prompt_error: OnceLock<dialoguer::Error>,
}

impl CliConfirmationClient {
    pub(crate) fn new(console: Console, mode: ConfirmationMode) -> Self {
        Self {
            console,
            mode,
            prompt_error: OnceLock::new(),
        }
    }

    pub(crate) fn into_prompt_error(self) -> Option<dialoguer::Error> {
        self.prompt_error.into_inner()
    }
}

impl ConfirmationClient for CliConfirmationClient {
    fn confirm(&self, confirmation: &Confirmation) -> bool {
        match self.mode {
            ConfirmationMode::AssumeYes => true,
            ConfirmationMode::Prompt => {
                match self.console.confirm(&confirmation_dialog(confirmation)) {
                    Ok(ConfirmationAnswer::Accepted) => true,
                    Ok(ConfirmationAnswer::Declined) => false,
                    Err(error) => {
                        drop(self.prompt_error.set(error));
                        false
                    }
                }
            }
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
                Detail::new("Task", &confirmation.task_identifier),
                Detail::new("Title", &confirmation.title),
                Detail::new("Status", confirmation.status),
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
                Detail::new("Task", &confirmation.task_identifier),
                Detail::new("Project", &confirmation.project),
                Detail::new(
                    "Completed",
                    optional_display(confirmation.completion_date.as_ref()),
                ),
                Detail::new(
                    "Commits",
                    optional_text(confirmation.commit_provenance.as_deref()),
                ),
                Detail::new("Report", optional_text(confirmation.report.as_deref())),
            ],
            "Delete this completion data and reopen the task?",
            ConfirmationDefault::No,
            ConfirmationTone::Destructive,
        ),
    }
}

fn optional_display(value: Option<&impl fmt::Display>) -> String {
    value.map_or_else(|| "(not recorded)".to_string(), ToString::to_string)
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
    use pwf_models::{
        AppDate,
        project::ProjectName,
        task::{TaskId, TaskStatus, TaskTitle},
    };
    use pwf_wire::{
        confirmation::{RemoveTaskConfirmation, ReopenTaskConfirmation},
        task::TaskNotePath,
    };

    use super::*;

    #[test]
    fn renders_aligned_plain_details_and_flattens_line_endings() {
        let dialog = ConfirmationDialog::new(
            "Confirm",
            vec![
                Detail::new("Task", "PWF-0001"),
                Detail::new("Long label", "first\nsecond\r\nthird"),
            ],
            "Proceed?",
            ConfirmationDefault::Yes,
            ConfirmationTone::Informational,
        );

        assert_eq!(
            dialog.render(false),
            "Confirm\n\n  Task        PWF-0001\n  Long label  first second third"
        );
    }

    #[test]
    fn cancellation_is_a_declined_answer() {
        assert_eq!(ConfirmationAnswer::from(None), ConfirmationAnswer::Declined);
    }

    #[test]
    fn removal_confirmation_renders_aligned_task_details() {
        let confirmation = Confirmation::RemoveTask(RemoveTaskConfirmation {
            task_identifier: TaskId::try_new("PWF-0001").unwrap(),
            project: ProjectName::try_new("pwf").unwrap(),
            title: TaskTitle::try_new("stale task").unwrap(),
            status: TaskStatus::Active,
            note_path: TaskNotePath::new("/notes/pwf/PWF-0001.md".into()),
        });

        assert_eq!(
            confirmation_dialog(&confirmation).render(false),
            "Confirm task removal\n\n  Task     PWF-0001\n  Title    stale task\n  Status   active\n  Project  pwf\n  Note     /notes/pwf/PWF-0001.md"
        );
    }

    #[test]
    fn reopen_confirmation_emphasizes_every_deleted_artifact() {
        let confirmation = Confirmation::ReopenTask(ReopenTaskConfirmation {
            task_identifier: TaskId::try_new("PWF-0001").unwrap(),
            project: ProjectName::try_new("pwf").unwrap(),
            completion_date: Some(AppDate::from_calendar_date(2026, 8, 17).unwrap()),
            commit_provenance: Some("abc..def".to_string()),
            report: Some("validated the release".to_string()),
        });

        assert_eq!(
            confirmation_dialog(&confirmation).render(false),
            "Reopen task and delete completion data\n\n  Task       PWF-0001\n  Project    pwf\n  Completed  2026-08-17\n  Commits    abc..def\n  Report     validated the release"
        );
    }
}
