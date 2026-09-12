//! Shared summaries, labeled fields, and confirmation presentation.

use std::fmt::{self, Write as _};

use anstyle::{AnsiColor, Color};
use dialoguer::{
    console::measure_text_width,
    theme::{ColorfulTheme, Theme},
};
use pwf_client::confirmation::Confirmation;
use pwf_models::settings::RgbColor;

pub(crate) fn render_summary(
    identifier: &str,
    title: &str,
    color: impl Into<Color>,
    color_on: bool,
) -> String {
    let identifier = if color_on {
        paint(identifier, color, true)
    } else {
        identifier.to_string()
    };
    format!("{identifier} :: {title}")
}

pub(crate) fn render_confirmation(
    label: &str,
    color: AnsiColor,
    id: &str,
    headline: &str,
    detail_lines: &[String],
    color_on: bool,
) -> String {
    let mut output = format!(
        "{label}: {}\n",
        paint(&format!("{id} {headline}"), color, color_on)
    );
    for line in detail_lines {
        output.push_str(line);
        output.push('\n');
    }
    output
}

pub(crate) const fn rgb_color(color: RgbColor) -> anstyle::RgbColor {
    anstyle::RgbColor(color.red(), color.green(), color.blue())
}

pub(crate) fn paint(text: &str, color: impl Into<Color>, enabled: bool) -> String {
    if !enabled {
        return format!("**{text}**");
    }
    highlight(text, color.into(), true)
}

fn highlight(text: &str, color: Color, enabled: bool) -> String {
    if !enabled {
        return text.to_string();
    }
    let style = anstyle::Style::new().bold().fg_color(Some(color));
    format!("{}{}{}", style.render(), text, style.render_reset())
}

pub(crate) fn render_fields(fields: &[Field], color: bool, columns: Option<usize>) -> String {
    let label_width = fields
        .iter()
        .map(|field| measure_text_width(field.label))
        .max()
        .unwrap_or_default();
    let value_column = label_width + 4;
    let width = columns.map(|columns| columns.saturating_sub(value_column).max(1));
    let mut output = String::new();
    for field in fields {
        let label = format!(
            "{}{}",
            field.label,
            " ".repeat(label_width - measure_text_width(field.label))
        );
        let lines = field_lines(&field.value, width);
        for (index, line) in lines.into_iter().enumerate() {
            if !output.is_empty() {
                output.push('\n');
            }
            if index == 0 {
                let _ = write!(
                    output,
                    "  {}  {line}",
                    highlight(&label, AnsiColor::Cyan.into(), color)
                );
            } else {
                let _ = write!(output, "{:value_column$}{line}", "");
            }
        }
    }
    output
}

fn field_lines(value: &str, width: Option<usize>) -> Vec<&str> {
    let Some(width) = width else {
        return vec![value];
    };
    let mut lines = Vec::new();
    let mut start = 0;
    let mut line_width = 0;
    let mut last_space = None;
    for (offset, character) in value.char_indices() {
        let mut buffer = [0; 4];
        let character_width = measure_text_width(character.encode_utf8(&mut buffer));
        if line_width > 0 && character_width > 0 && line_width + character_width > width {
            let split = last_space.take().unwrap_or(offset);
            lines.push(&value[start..split]);
            start = split;
            line_width = measure_text_width(&value[start..offset]);
        }
        line_width += character_width;
        if character.is_whitespace() {
            last_space = Some(offset + character.len_utf8());
        }
    }
    lines.push(&value[start..]);
    lines
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ConfirmationDefault {
    Yes,
    No,
}

impl ConfirmationDefault {
    pub(crate) fn accepted(self) -> bool {
        matches!(self, Self::Yes)
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

pub(crate) struct Field {
    label: &'static str,
    value: String,
}

impl Field {
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
    details: Vec<Field>,
    pub(crate) question: &'static str,
    pub(crate) default: ConfirmationDefault,
    tone: ConfirmationTone,
}

impl ConfirmationDialog {
    pub(crate) fn new(
        heading: &'static str,
        details: Vec<Field>,
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

    pub(crate) fn render(&self, color: bool, columns: Option<usize>) -> String {
        format!(
            "{}\n\n{}",
            highlight(self.heading, self.tone.heading_color().into(), color),
            render_fields(&self.details, color, columns)
        )
    }
}

pub(crate) fn confirmation_dialog(confirmation: &Confirmation) -> ConfirmationDialog {
    match confirmation {
        Confirmation::DeleteNote(confirmation) => ConfirmationDialog::new(
            "Confirm note removal",
            vec![
                Field::new("Note", &confirmation.note_id),
                Field::new("Title", &confirmation.title),
                Field::new("Project", &confirmation.project),
            ],
            "Remove this note?",
            ConfirmationDefault::No,
            ConfirmationTone::Destructive,
        ),
        Confirmation::DeleteTask(confirmation) => ConfirmationDialog::new(
            "Confirm task removal",
            {
                let mut details = vec![
                    Field::new("Task", &confirmation.task_id),
                    Field::new("Title", &confirmation.title),
                    Field::new("Status", task_status(confirmation.status)),
                    Field::new("Project", &confirmation.project),
                    Field::new("Path", &confirmation.file_path),
                ];
                if let Some(vault) = &confirmation.obsidian_vault {
                    details.push(Field::new("Obsidian vault", vault));
                }
                details.push(Field::new(
                    "Deletion",
                    confirmation.trash_folder.as_ref().map_or_else(
                        || "hard delete".to_string(),
                        |folder| format!("moves to {folder}"),
                    ),
                ));
                details
            },
            "Remove this task?",
            ConfirmationDefault::No,
            ConfirmationTone::Destructive,
        ),
        Confirmation::ActivateTask(confirmation) => ConfirmationDialog::new(
            "Activate task and delete completion data",
            vec![
                Field::new("Task", &confirmation.task_id),
                Field::new("Project", &confirmation.project),
                Field::new(
                    "Completed",
                    optional_text(confirmation.completion_date.as_deref()),
                ),
                Field::new("Commits", optional_text(confirmation.commits.as_deref())),
                Field::new("Report", optional_text(confirmation.report.as_deref())),
            ],
            "Delete this completion data and activate the task?",
            ConfirmationDefault::No,
            ConfirmationTone::Destructive,
        ),
        Confirmation::DispatchSession(preflight) => {
            let confirmation = preflight.confirmation.as_ref();
            let identity = confirmation.map_or("(unknown)", |value| value.session_name.as_str());
            let is_compound = confirmation.is_some_and(|value| value.task_ids.len() > 1);
            let task_label = if is_compound { "Tasks" } else { "Task" };
            let mut details = vec![Field::new(task_label, identity)];
            if !is_compound {
                details.push(Field::new(
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
    match pwf_client::pb::TaskStatus::try_from(value).ok() {
        Some(pwf_client::pb::TaskStatus::Active) => "active",
        Some(pwf_client::pb::TaskStatus::Done) => "done",
        Some(pwf_client::pb::TaskStatus::Backlog) => "backlog",
        Some(pwf_client::pb::TaskStatus::Cancelled) => "cancelled",
        Some(pwf_client::pb::TaskStatus::Unspecified) | None => "unspecified",
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

#[derive(Default)]
pub(crate) struct ConfirmationTheme(ColorfulTheme);

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
    use pwf_client::pb::{
        ActivateTaskConfirmation, DeleteNoteConfirmation, DeleteTaskConfirmation, TaskStatus,
    };

    use super::*;

    #[test]
    fn renders_aligned_plain_details_and_flattens_line_endings() {
        let dialog = ConfirmationDialog::new(
            "Confirm",
            vec![
                Field::new("Task", "FOO-0001"),
                Field::new("Long label", "first\nsecond\r\nthird"),
            ],
            "Proceed?",
            ConfirmationDefault::Yes,
            ConfirmationTone::Informational,
        );

        assert_eq!(
            dialog.render(false, None),
            "Confirm\n\n  Task        FOO-0001\n  Long label  first second third"
        );
    }

    #[test]
    fn removal_confirmation_renders_aligned_task_details() {
        let confirmation = Confirmation::DeleteTask(DeleteTaskConfirmation {
            obsidian_vault: None,
            trash_folder: None,
            task_id: "FOO-0001".to_string(),
            project: "foo".to_string(),
            title: "stale task".to_string(),
            status: TaskStatus::Active as i32,
            file_path: "/notes/foo/FOO-0001.md".to_string(),
        });

        assert_eq!(
            confirmation_dialog(&confirmation).render(false, None),
            "Confirm task removal\n\n  Task      FOO-0001\n  Title     stale task\n  Status    active\n  Project   foo\n  Path      /notes/foo/FOO-0001.md\n  Deletion  hard delete"
        );
    }

    #[test]
    fn note_removal_uses_the_destructive_default_no_dialog() {
        let confirmation = Confirmation::DeleteNote(DeleteNoteConfirmation {
            note_id: "FOO-NOTE-0001".to_string(),
            project: "foo".to_string(),
            title: "stale insight".to_string(),
        });

        let dialog = confirmation_dialog(&confirmation);
        assert_eq!(dialog.default, ConfirmationDefault::No);
        assert_eq!(dialog.tone, ConfirmationTone::Destructive);
        assert_eq!(
            dialog.render(false, None),
            "Confirm note removal\n\n  Note     FOO-NOTE-0001\n  Title    stale insight\n  Project  foo"
        );
    }

    #[test]
    fn activate_confirmation_emphasizes_every_deleted_artifact() {
        let confirmation = Confirmation::ActivateTask(ActivateTaskConfirmation {
            task_id: "FOO-0001".to_string(),
            project: "foo".to_string(),
            completion_date: Some("2026-08-17".to_string()),
            commits: Some("abc..def".to_string()),
            report: Some("validated the release".to_string()),
        });

        assert_eq!(
            confirmation_dialog(&confirmation).render(false, None),
            "Activate task and delete completion data\n\n  Task       FOO-0001\n  Project    foo\n  Completed  2026-08-17\n  Commits    abc..def\n  Report     validated the release"
        );
    }

    #[test]
    fn plain_summaries_remain_raw_while_mutations_use_emphasis() {
        assert_eq!(
            render_summary("FOO-NOTE-0001", "sample note", AnsiColor::Yellow, false),
            "FOO-NOTE-0001 :: sample note"
        );
        assert_eq!(
            render_confirmation(
                "Added pwf note",
                AnsiColor::Green,
                "FOO-NOTE-0001",
                "foo :: sample note",
                &[],
                false,
            ),
            "Added pwf note: **FOO-NOTE-0001 foo :: sample note**\n"
        );
    }

    #[test]
    fn fields_wrap_paths_and_wide_text_under_the_value_column() {
        let fields = [
            Field::new("Path", "/abcdefghijk"),
            Field::new("Tags", "日本語界"),
        ];
        let output = render_fields(&fields, false, Some(14));
        assert_eq!(
            output,
            "  Path  /abcde\n        fghijk\n  Tags  日本語\n        界"
        );
        assert!(output.lines().all(|line| measure_text_width(line) <= 14));
    }

    #[test]
    fn fields_preserve_words_and_unwrapped_values() {
        let fields = [Field::new("Title", "one two three four")];
        assert_eq!(
            render_fields(&fields, false, Some(17)),
            "  Title  one two \n         three \n         four"
        );
        assert_eq!(
            render_fields(&fields, false, None),
            "  Title  one two three four"
        );
    }

    #[test]
    fn field_highlights_do_not_change_alignment() {
        let fields = [
            Field::new("Path", "/abc"),
            Field::new("Created", "12/09/2026 01:38"),
        ];
        let plain = render_fields(&fields, false, None);
        let colored = render_fields(&fields, true, None);
        assert_eq!(dialoguer::console::strip_ansi_codes(&colored), plain);
        let style = anstyle::Style::new()
            .bold()
            .fg_color(Some(AnsiColor::Cyan.into()));
        assert!(colored.contains(&format!("  {style}Created{style:#}  12/09/2026 01:38")));
    }
}
