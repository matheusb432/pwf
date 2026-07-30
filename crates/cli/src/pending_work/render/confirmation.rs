//! Formats typed mutation confirmations at their owning leaf boundaries.

use anstyle::AnsiColor;
use pwf_application::pending_work::{AddPendingWorkItemOk, RemovedItem, UpdatePendingWorkItemOk};

use super::paint;

pub(in crate::pending_work) fn render_added(item: &AddPendingWorkItemOk, color_on: bool) -> String {
    render_confirmation(
        "Added pwf task",
        AnsiColor::Green,
        &item.id,
        &added_headline(item),
        &[format!("  file: {}", item.note_path.display())],
        color_on,
    )
}

pub(in crate::pending_work) fn render_removed(item: &RemovedItem, color_on: bool) -> String {
    render_confirmation(
        "Removed pwf task",
        AnsiColor::Red,
        &item.id,
        &format!("{} :: {}", item.project, item.title),
        &[
            format!("  deleted: {}", item.deleted_path.display()),
            format!("  unlinked: {}", item.unlinked),
        ],
        color_on,
    )
}

pub(in crate::pending_work) fn render_updated(
    item: &UpdatePendingWorkItemOk,
    color_on: bool,
) -> String {
    let (id, headline) = match item {
        UpdatePendingWorkItemOk::OpenItemEdit {
            id, project, title, ..
        } => (id.as_str(), format!("{project} :: {title}")),
        UpdatePendingWorkItemOk::Changed { id, changes } => (id.as_str(), changes.join(", ")),
    };
    render_confirmation(
        "Updated pwf task",
        AnsiColor::Blue,
        id,
        &headline,
        &[],
        color_on,
    )
}

pub(super) fn render_review_item(item: &AddPendingWorkItemOk) -> String {
    format!(
        "ADDED PWF TASK [{}] {}\n  file: {}\n",
        item.id,
        added_headline(item),
        item.note_path.display()
    )
}

fn added_headline(item: &AddPendingWorkItemOk) -> String {
    format!("{} :: {}", item.project, item.title)
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
    use std::path::PathBuf;

    use super::*;

    fn added_item() -> AddPendingWorkItemOk {
        AddPendingWorkItemOk {
            id: "PWF-0087".to_string(),
            project: "pwf".to_string(),
            title: "color tui output when adding pwf task".to_string(),
            note_path: PathBuf::from("/x/PWF-0087.md"),
            created_section: None,
        }
    }

    #[test]
    fn added_plain_prefixes_label_with_no_leading_blank_line() {
        assert_eq!(
            render_added(&added_item(), false),
            "Added pwf task: **PWF-0087 pwf :: color tui output when adding pwf task**\n  file: /x/PWF-0087.md\n"
        );
    }

    #[test]
    fn added_colored_keeps_identifier_and_detail_intact() {
        let output = render_added(&added_item(), true);

        assert!(output.starts_with("Added pwf task: "), "got: {output}");
        assert!(!output.starts_with('\n'), "got: {output}");
        assert!(output.contains('\u{1b}'), "got: {output}");
        assert!(output.contains("PWF-0087"), "got: {output}");
        assert!(
            output.contains("pwf :: color tui output when adding pwf task"),
            "got: {output}"
        );
        assert!(output.contains("  file: /x/PWF-0087.md\n"));
    }

    #[test]
    fn review_item_preserves_the_existing_added_line() {
        assert_eq!(
            render_review_item(&added_item()),
            "ADDED PWF TASK [PWF-0087] pwf :: color tui output when adding pwf task\n  file: /x/PWF-0087.md\n"
        );
    }

    #[test]
    fn removed_plain_preserves_confirmation_shape() {
        let item = RemovedItem {
            id: "PWF-0002".to_string(),
            project: "pwf".to_string(),
            title: "stale task".to_string(),
            deleted_path: PathBuf::from("/x.md"),
            unlinked: "/x.md".to_string(),
        };

        assert_eq!(
            render_removed(&item, false),
            "Removed pwf task: **PWF-0002 pwf :: stale task**\n  deleted: /x.md\n  unlinked: /x.md\n"
        );
    }

    #[test]
    fn updated_variants_preserve_confirmation_shape() {
        let open = UpdatePendingWorkItemOk::OpenItemEdit {
            id: "PWF-0003".to_string(),
            project: "pwf".to_string(),
            title: "renamed".to_string(),
        };
        let closed = UpdatePendingWorkItemOk::Changed {
            id: "PWF-0004".to_string(),
            changes: vec!["commits: abc..def".to_string()],
        };

        assert_eq!(
            render_updated(&open, false),
            "Updated pwf task: **PWF-0003 pwf :: renamed**\n"
        );
        assert_eq!(
            render_updated(&closed, false),
            "Updated pwf task: **PWF-0004 commits: abc..def**\n"
        );
    }
}
