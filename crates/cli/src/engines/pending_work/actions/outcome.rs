// Typed outcomes for engine verbs are domain-owned; this module keeps only the
// frozen legacy text rendering and the CLI-edge `EngineOutcome` wrapper.

pub(crate) use pwf_domain::pending_work::AddedItem;
pub(in crate::engines::pending_work) use pwf_domain::pending_work::{
    MutationOutcome, RemovedItem, UpdatedItem,
};

pub(super) trait OutcomeRender {
    fn id(&self) -> &str;
    fn headline(&self) -> String;
    fn detail_lines(&self) -> Vec<String>;

    fn raw_text(&self, verb: &str) -> String {
        render_raw_text(verb, self.id(), &self.headline(), &self.detail_lines())
    }
}

impl OutcomeRender for AddedItem {
    fn id(&self) -> &str {
        &self.id
    }

    fn headline(&self) -> String {
        format!("{} :: {}", self.project, self.title)
    }

    fn detail_lines(&self) -> Vec<String> {
        vec![format!("  file: {}", self.note_path.display())]
    }
}

impl OutcomeRender for RemovedItem {
    fn id(&self) -> &str {
        &self.id
    }

    fn headline(&self) -> String {
        format!("{} :: {}", self.project, self.title)
    }

    fn detail_lines(&self) -> Vec<String> {
        vec![
            format!("  deleted: {}", self.deleted_path.display()),
            format!("  unlinked: {}", self.unlinked),
        ]
    }
}

impl OutcomeRender for UpdatedItem {
    fn id(&self) -> &str {
        match self {
            UpdatedItem::OpenItemEdit { id, .. } | UpdatedItem::Changed { id, .. } => id,
        }
    }

    fn headline(&self) -> String {
        match self {
            UpdatedItem::OpenItemEdit { project, title, .. } => format!("{project} :: {title}"),
            UpdatedItem::Changed { changes, .. } => changes.join(", "),
        }
    }

    fn detail_lines(&self) -> Vec<String> {
        Vec::new()
    }
}

/// Compose the frozen `"<VERB> PWF TASK [{id}] {headline}\n{detail lines}"`
/// shape shared by every outcome's `raw_text()`. `render_outcome_confirmation`
/// (`confirm_render.rs`) mirrors this same `headline`/`detail_lines` pair for
/// the presentation-layer rendering, so the two never drift independently.
fn render_raw_text(verb: &str, id: &str, headline: &str, detail_lines: &[String]) -> String {
    let mut out = format!("{verb} PWF TASK [{id}] {headline}\n");
    for line in detail_lines {
        out.push_str(line);
        out.push('\n');
    }
    out
}

pub(super) fn mutation_raw_text(outcome: &MutationOutcome) -> String {
    match outcome {
        MutationOutcome::Added(added) => added.raw_text("ADDED"),
        MutationOutcome::Removed(removed) => removed.raw_text("REMOVED"),
        MutationOutcome::Updated(updated) => updated.raw_text("UPDATED"),
    }
}

/// What one engine verb produced. Raw legacy text is a rendering of this,
/// applied at the seams that still need plain text — the `run_args` seam,
/// `run()`'s own `Text` passthrough, and `done.rs`'s direct `raw_text()` call
/// for the `--review` embed; `run()` renders confirmations from the typed
/// variants directly for every other case.
#[derive(Debug)]
pub(in crate::engines::pending_work) enum EngineOutcome {
    Mutation(MutationOutcome),
    Text(String),
}

impl EngineOutcome {
    /// The legacy seam string: the matching variant's `raw_text()`, or the
    /// text verbatim.
    pub(in crate::engines::pending_work) fn into_raw_text(self) -> String {
        match self {
            EngineOutcome::Mutation(outcome) => mutation_raw_text(&outcome),
            EngineOutcome::Text(text) => text,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use pwf_domain::pending_work::{AddedItem, MutationOutcome, RemovedItem, UpdatedItem};

    use super::*;

    #[test]
    fn added_item_raw_text_matches_legacy_shape() {
        let item = AddedItem {
            id: "PWF-0001".to_string(),
            project: "pwf".to_string(),
            title: "do it".to_string(),
            note_path: PathBuf::from("/x/PWF-0001.md"),
            created_section: None,
        };

        assert_eq!(
            item.raw_text("ADDED"),
            "ADDED PWF TASK [PWF-0001] pwf :: do it\n  file: /x/PWF-0001.md\n"
        );
    }

    #[test]
    fn mutation_outcome_into_raw_text_uses_domain_added_variant() {
        let outcome = EngineOutcome::Mutation(MutationOutcome::Added(AddedItem {
            id: "PWF-0001".to_string(),
            project: "pwf".to_string(),
            title: "do it".to_string(),
            note_path: PathBuf::from("/x/PWF-0001.md"),
            created_section: None,
        }));

        assert_eq!(
            outcome.into_raw_text(),
            "ADDED PWF TASK [PWF-0001] pwf :: do it\n  file: /x/PWF-0001.md\n"
        );
    }

    #[test]
    fn text_outcome_into_raw_text_passes_text_through() {
        let outcome = EngineOutcome::Text("some text\n".to_string());

        assert_eq!(outcome.into_raw_text(), "some text\n");
    }

    #[test]
    fn removed_item_raw_text_matches_legacy_shape() {
        let item = RemovedItem {
            id: "PWF-0002".to_string(),
            project: "pwf".to_string(),
            title: "stale task".to_string(),
            deleted_path: PathBuf::from("/x.md"),
            unlinked: "/x/pwf.md".to_string(),
        };

        assert_eq!(
            item.raw_text("REMOVED"),
            "REMOVED PWF TASK [PWF-0002] pwf :: stale task\n  deleted: /x.md\n  unlinked: /x/pwf.md\n"
        );
    }

    #[test]
    fn updated_item_open_item_edit_raw_text_matches_legacy_shape() {
        let item = UpdatedItem::OpenItemEdit {
            id: "PWF-0003".to_string(),
            project: "pwf".to_string(),
            title: "renamed".to_string(),
        };

        assert_eq!(
            item.raw_text("UPDATED"),
            "UPDATED PWF TASK [PWF-0003] pwf :: renamed\n"
        );
    }

    #[test]
    fn updated_item_changed_raw_text_matches_legacy_shape() {
        let item = UpdatedItem::Changed {
            id: "PWF-0004".to_string(),
            changes: vec![
                "commits: abc..def".to_string(),
                "report appended".to_string(),
            ],
        };

        assert_eq!(
            item.raw_text("UPDATED"),
            "UPDATED PWF TASK [PWF-0004] commits: abc..def, report appended\n"
        );
    }
}
