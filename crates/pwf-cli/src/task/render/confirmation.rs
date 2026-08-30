//! Formats typed mutation confirmations at their owning leaf boundaries.

use anstyle::AnsiColor;
use pwf_client::v1::{CreatedTask, DeletedTask, UpdatedTask};

use super::paint;

pub(in crate::task) fn render_added(task: &CreatedTask, color_on: bool) -> String {
    render_confirmation(
        "Added pwf task",
        AnsiColor::Green,
        &task.id,
        &added_headline(task),
        &[format!("  file: {}", task.note_path)],
        color_on,
    )
}

pub(in crate::task) fn render_removed(task: &DeletedTask, color_on: bool) -> String {
    render_confirmation(
        "Removed pwf task",
        AnsiColor::Red,
        &task.id,
        &format!("{} :: {}", task.project, task.title),
        &[
            format!("  deleted: {}", task.deleted_path),
            format!(
                "  unlinked: {}",
                task.unlinked
                    .as_ref()
                    .map_or_else(String::new, ToString::to_string)
            ),
        ],
        color_on,
    )
}

pub(in crate::task) fn render_edited(task: &UpdatedTask, color_on: bool) -> String {
    render_confirmation(
        "Edited pwf task",
        AnsiColor::Blue,
        &task.id,
        &format!("{} :: {}", task.project, task.title),
        &[],
        color_on,
    )
}

pub(super) fn render_review_task(task: &CreatedTask) -> String {
    format!(
        "ADDED PWF TASK [{}] {}\n  file: {}\n",
        task.id,
        added_headline(task),
        task.note_path
    )
}

fn added_headline(task: &CreatedTask) -> String {
    format!("{} :: {}", task.project, task.title)
}

fn render_confirmation(
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

#[cfg(test)]
mod tests {
    use super::*;

    fn added_task() -> CreatedTask {
        CreatedTask {
            id: "FOO-0001".to_string(),
            project: "foo".to_string(),
            title: "sample task".to_string(),
            note_path: "/x/FOO-0001.md".to_string(),
            created_section: None,
        }
    }

    #[test]
    fn added_plain_prefixes_label_with_no_leading_blank_line() {
        assert_eq!(
            render_added(&added_task(), false),
            "Added pwf task: **FOO-0001 foo :: sample task**\n  file: /x/FOO-0001.md\n"
        );
    }

    #[test]
    fn review_task_preserves_the_existing_added_line() {
        assert_eq!(
            render_review_task(&added_task()),
            "ADDED PWF TASK [FOO-0001] foo :: sample task\n  file: /x/FOO-0001.md\n"
        );
    }

    #[test]
    fn removed_plain_preserves_confirmation_shape() {
        let task = DeletedTask {
            id: "FOO-0002".to_string(),
            project: "foo".to_string(),
            title: "stale task".to_string(),
            deleted_path: "/x.md".to_string(),
            unlinked: Some("/x.md".to_string()),
        };

        assert_eq!(
            render_removed(&task, false),
            "Removed pwf task: **FOO-0002 foo :: stale task**\n  deleted: /x.md\n  unlinked: /x.md\n"
        );
    }

    #[test]
    fn edited_plain_uses_the_edit_verb() {
        let task = UpdatedTask {
            id: "FOO-0002".to_string(),
            project: "foo".to_string(),
            title: "edited task".to_string(),
        };

        assert_eq!(
            render_edited(&task, false),
            "Edited pwf task: **FOO-0002 foo :: edited task**\n"
        );
    }
}
