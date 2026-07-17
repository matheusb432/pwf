//! Formats typed mutation confirmations only for direct terminal output.
//! `run_typed` and `run_args` retain raw text; external allocators own `ADDED PWF TASK [<id>]`.

use anstyle::AnsiColor;
use pwf_domain::pending_work::MutationOutcome;

use super::{
    super::color::paint,
    outcome::{EngineOutcome, OutcomeRender},
};

fn mutation_confirmation_parts(
    outcome: &MutationOutcome,
) -> (&'static str, AnsiColor, &str, String, Vec<String>) {
    match outcome {
        MutationOutcome::Added(item) => (
            "Added pwf task",
            AnsiColor::Green,
            item.id.as_str(),
            item.headline(),
            item.detail_lines(),
        ),
        MutationOutcome::Removed(item) => (
            "Removed pwf task",
            AnsiColor::Red,
            item.id.as_str(),
            item.headline(),
            item.detail_lines(),
        ),
        MutationOutcome::Updated(updated) => (
            "Updated pwf task",
            AnsiColor::Blue,
            updated.id(),
            updated.headline(),
            updated.detail_lines(),
        ),
    }
}

pub(in crate::engines::pending_work) fn render_outcome_confirmation(
    outcome: &EngineOutcome,
    on: bool,
) -> Option<String> {
    let (label, color, id, headline, detail_lines) = match outcome {
        EngineOutcome::Mutation(outcome) => mutation_confirmation_parts(outcome),
        EngineOutcome::Text(_) => return None,
    };

    let mut out = format!(
        "{label}: {}\n",
        paint(&format!("{id} {headline}"), color, on)
    );
    for line in detail_lines {
        out.push_str(&line);
        out.push('\n');
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use pwf_domain::pending_work::{AddedItem, MutationOutcome, RemovedItem, UpdatedItem};

    use super::*;

    #[test]
    fn plain_prefixes_label_with_no_leading_blank_line() {
        let outcome = EngineOutcome::Mutation(MutationOutcome::Added(AddedItem {
            id: "PWF-0087".to_string(),
            project: "pwf".to_string(),
            title: "color tui output when adding pwf task".to_string(),
            note_path: PathBuf::from("/x/PWF-0087.md"),
            created_section: None,
        }));

        let out = render_outcome_confirmation(&outcome, false).unwrap();

        assert_eq!(
            out,
            "Added pwf task: **PWF-0087 pwf :: color tui output when adding pwf task**\n  file: /x/PWF-0087.md\n"
        );
    }

    #[test]
    fn colored_has_no_leading_blank_line_and_keeps_id_and_rest_intact() {
        let outcome = EngineOutcome::Mutation(MutationOutcome::Added(AddedItem {
            id: "PWF-0087".to_string(),
            project: "pwf".to_string(),
            title: "color tui output when adding pwf task".to_string(),
            note_path: PathBuf::from("/x/PWF-0087.md"),
            created_section: None,
        }));

        let out = render_outcome_confirmation(&outcome, true).unwrap();

        assert!(out.starts_with("Added pwf task: "), "got: {out}");
        assert!(!out.starts_with('\n'), "got: {out}");
        assert!(out.contains('\u{1b}'), "got: {out}");
        assert!(out.contains("PWF-0087"), "got: {out}");
        assert!(
            out.contains("pwf :: color tui output when adding pwf task"),
            "got: {out}"
        );
        assert!(out.contains("  file: /x/PWF-0087.md\n"), "got: {out}");
    }

    #[test]
    fn removed_outcome_renders_with_removed_label_and_color() {
        let outcome = EngineOutcome::Mutation(MutationOutcome::Removed(RemovedItem {
            id: "PWF-0002".to_string(),
            project: "pwf".to_string(),
            title: "stale task".to_string(),
            deleted_path: PathBuf::from("/x.md"),
            unlinked: "/x.md".to_string(),
        }));

        let out = render_outcome_confirmation(&outcome, false).unwrap();

        assert_eq!(
            out,
            "Removed pwf task: **PWF-0002 pwf :: stale task**\n  deleted: /x.md\n  unlinked: /x.md\n"
        );
    }

    #[test]
    fn updated_open_item_edit_outcome_renders_with_updated_label_and_color() {
        let outcome =
            EngineOutcome::Mutation(MutationOutcome::Updated(UpdatedItem::OpenItemEdit {
                id: "PWF-0003".to_string(),
                project: "pwf".to_string(),
                title: "renamed".to_string(),
            }));

        let out = render_outcome_confirmation(&outcome, false).unwrap();

        assert_eq!(out, "Updated pwf task: **PWF-0003 pwf :: renamed**\n");
    }

    #[test]
    fn updated_changed_outcome_renders_with_updated_label_and_color() {
        let outcome = EngineOutcome::Mutation(MutationOutcome::Updated(UpdatedItem::Changed {
            id: "PWF-0004".to_string(),
            changes: vec!["commits: abc..def".to_string()],
        }));

        let out = render_outcome_confirmation(&outcome, false).unwrap();

        assert_eq!(out, "Updated pwf task: **PWF-0004 commits: abc..def**\n");
    }

    #[test]
    fn text_outcome_has_no_confirmation_rendering() {
        let outcome = EngineOutcome::Text("some text\n".to_string());

        assert_eq!(render_outcome_confirmation(&outcome, false), None);
    }
}
