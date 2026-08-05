//! Formats typed mutation confirmations at their owning leaf boundaries.

use anstyle::AnsiColor;
use pwf_application::task::{AddTaskOk, EditTaskOk, RemovedTask};
use pwf_models::task::TaskId;

use super::paint;

pub(in crate::task) fn render_added(task: &AddTaskOk, color_on: bool) -> String {
    render_confirmation(
        "Added pwf task",
        AnsiColor::Green,
        &task.id,
        &added_headline(task),
        &[format!("  file: {}", task.note_path.display())],
        color_on,
    )
}

pub(in crate::task) fn render_removed(task: &RemovedTask, color_on: bool) -> String {
    render_confirmation(
        "Removed pwf task",
        AnsiColor::Red,
        &task.id,
        &format!("{} :: {}", task.project, task.title),
        &[
            format!("  deleted: {}", task.deleted_path.display()),
            format!("  unlinked: {}", task.unlinked),
        ],
        color_on,
    )
}

pub(in crate::task) fn render_edited(task: &EditTaskOk, color_on: bool) -> String {
    render_confirmation(
        "Edited pwf task",
        AnsiColor::Blue,
        &task.id,
        &format!("{} :: {}", task.project, task.title),
        &[],
        color_on,
    )
}

pub(super) fn render_review_task(task: &AddTaskOk) -> String {
    format!(
        "ADDED PWF TASK [{}] {}\n  file: {}\n",
        task.id,
        added_headline(task),
        task.note_path.display()
    )
}

fn added_headline(task: &AddTaskOk) -> String {
    format!("{} :: {}", task.project, task.title)
}

fn render_confirmation(
    label: &str,
    color: AnsiColor,
    id: &TaskId,
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn added_task() -> AddTaskOk {
        AddTaskOk {
            id: TaskId::try_new("PWF-0087").unwrap(),
            project: "pwf".to_string(),
            title: "color tui output when adding pwf task".to_string(),
            note_path: PathBuf::from("/x/PWF-0087.md"),
            created_section: None,
        }
    }

    #[test]
    fn added_plain_prefixes_label_with_no_leading_blank_line() {
        assert_eq!(
            render_added(&added_task(), false),
            "Added pwf task: **PWF-0087 pwf :: color tui output when adding pwf task**\n  file: /x/PWF-0087.md\n"
        );
    }

    #[test]
    fn review_task_preserves_the_existing_added_line() {
        assert_eq!(
            render_review_task(&added_task()),
            "ADDED PWF TASK [PWF-0087] pwf :: color tui output when adding pwf task\n  file: /x/PWF-0087.md\n"
        );
    }

    #[test]
    fn removed_plain_preserves_confirmation_shape() {
        let task = RemovedTask {
            id: TaskId::try_new("PWF-0002").unwrap(),
            project: "pwf".to_string(),
            title: "stale task".to_string(),
            deleted_path: PathBuf::from("/x.md"),
            unlinked: "/x.md".to_string(),
        };

        assert_eq!(
            render_removed(&task, false),
            "Removed pwf task: **PWF-0002 pwf :: stale task**\n  deleted: /x.md\n  unlinked: /x.md\n"
        );
    }

    #[test]
    fn edited_plain_uses_the_edit_verb() {
        let task = EditTaskOk {
            id: TaskId::try_new("PWF-0002").unwrap(),
            project: "pwf".to_string(),
            title: "edited task".to_string(),
        };

        assert_eq!(
            render_edited(&task, false),
            "Edited pwf task: **PWF-0002 pwf :: edited task**\n"
        );
    }
}
