use std::path::Path;

use pwf_application::{
    AppDbStore, IndexEntry, PendingWorkItem, pending_work::remove::RemovePendingWorkItem,
};

use super::{
    super::{errors::PendingWorkError, model::Item, query::find_pending_item},
    outcome::{EngineOutcome, MutationOutcome},
};
use crate::{
    cli::EngineArgs,
    config::Config,
    confirm::{Confirmation, DefaultAnswer},
    confirm_prompt::{ConfirmationPrompt, Field},
    engines::handoff::mirror,
};

/// Includes both files when removal will also delete a linked handoff.
fn confirmation_question(item: &Item, item_path: &Path, handoff_path: Option<&Path>) -> String {
    let mut fields = vec![
        Field::new("task_id", item.id.clone()),
        Field::new("project", item.project.clone()),
        Field::new("title", item.session.clone()),
        Field::new("note", item_path.display().to_string()),
    ];
    if let Some(path) = handoff_path {
        fields.push(Field::new("handoff", path.display().to_string()));
    }
    ConfirmationPrompt::new(
        "Confirm task removal",
        &fields,
        "Remove this pending-work task?",
    )
    .to_string()
}

pub(in crate::engines::pending_work) fn run_remove<S>(
    cfg: &Config,
    store: &S,
    args: &EngineArgs,
    confirmation: &impl Fn(&str, DefaultAnswer) -> Confirmation,
) -> Result<EngineOutcome, PendingWorkError>
where
    S: AppDbStore<PendingWorkItem> + AppDbStore<IndexEntry>,
{
    let id = args
        .id
        .as_deref()
        .ok_or(PendingWorkError::MissingId { action: "remove" })?;

    let registry = crate::engines::pending_work::run::project_registry(cfg);
    let item = find_pending_item(store, &registry, id)?;
    let item_file = item
        .file_path
        .as_deref()
        .ok_or(PendingWorkError::RemoveRequiresFileModel)?;
    let item_path = Path::new(item_file);
    if !item_path.exists() {
        return Err(PendingWorkError::WorkItemNoteMissing {
            path: item_path.to_path_buf(),
        });
    }

    // Preflight the linked handoff before prompting so failures leave both files untouched.
    let gate = mirror::handoff_gate(cfg, store, &registry, id)?;
    let handoff_path = gate.as_ref().map(mirror::preflight_delete).transpose()?;

    // Interactive removal defaults to yes; `--yes` and non-interactive calls skip the prompt.
    if !args.assume_yes {
        let question = confirmation_question(&item, item_path, handoff_path.as_deref());
        if confirmation(&question, DefaultAnswer::Yes) == Confirmation::Declined {
            return Ok(EngineOutcome::Text(format!(
                "# remove {} — aborted\nnothing deleted.\n",
                item.id
            )));
        }
    }

    let removed = pwf_application::pending_work::remove::execute(
        &RemovePendingWorkItem {
            id: item.id.clone(),
        },
        store,
        &registry,
    )
    .map_err(|error| PendingWorkError::ApplicationWrite(error.to_string()))?;

    if let Some(g) = gate {
        let path = mirror::delete_for_item(&g).map_err(|source| {
            PendingWorkError::HandoffMirrorAfterMutation {
                id: g.id.clone(),
                source,
                remedy: super::super::errors::REMOVE_MIRROR_REMEDY.to_string(),
            }
        })?;
        eprintln!("info: removed handoff {}", path.display());
    }

    Ok(EngineOutcome::Mutation(MutationOutcome::Removed(removed)))
}

#[cfg(test)]
mod tests {
    use std::{assert_matches, path::PathBuf};

    use super::*;
    use crate::engines::pending_work::errors::PendingWorkError;

    fn accepted(_: &str, _: DefaultAnswer) -> Confirmation {
        Confirmation::Accepted
    }

    fn declined(_: &str, _: DefaultAnswer) -> Confirmation {
        Confirmation::Declined
    }

    fn noninteractive(_: &str, _: DefaultAnswer) -> Confirmation {
        Confirmation::NonInteractive
    }

    fn cfg(notes: &Path) -> Config {
        crate::config::from_json(
            &format!(
                r#"{{ "notesDir": "{}", "projects": {{ "glep-shimeji": "/repo" }}, "prefixes": {{ "glep-shimeji": "GLP" }} }}"#,
                notes.to_string_lossy().replace('\\', "\\\\")
            ),
            None,
        )
        .unwrap()
    }

    fn stage_file_item(index: &str) -> (tempfile::TempDir, Config) {
        let stage = tempfile::tempdir().unwrap();
        let notes = stage.path().join("notes");
        let project = notes.join("glep-shimeji");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("GLP-0001.md"),
            "---\nid: GLP-0001\nstatus: active\ntitle: tray gui\nproject: glep-shimeji\ncreated: 2026-01-01\n---\n\nbody\n",
        )
        .unwrap();
        std::fs::write(
            project.join("glep-shimeji.md"),
            format!("---\nid: glp\ntitle: glep-shimeji\n---\n\n{index}"),
        )
        .unwrap();
        let cfg = cfg(&notes);
        (stage, cfg)
    }

    #[test]
    fn missing_id_returns_typed_error_with_legacy_display() {
        let (_stage, cfg) = stage_file_item("- [ ] [[GLP-0001]]\n");

        let store = crate::engines::pending_work::store_for(&cfg);
        let err = run_remove(&cfg, &store, &EngineArgs::default(), &noninteractive).unwrap_err();

        assert_matches!(
            err,
            PendingWorkError::MissingId { action } if action == "remove"
        );
        assert_eq!(err.to_string(), "--id is required for remove.");
    }

    #[test]
    fn missing_item_file_returns_typed_error_with_legacy_display() {
        let stage = tempfile::tempdir().unwrap();
        let notes = stage.path().join("notes");
        let project = notes.join("glep-shimeji");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("glep-shimeji.md"),
            "---\nid: glp\ntitle: glep-shimeji\n---\n\n- [ ] `legacy` :: do it\n",
        )
        .unwrap();
        let cfg = cfg(&notes);
        let args = EngineArgs {
            id: Some("glep-shimeji:1".to_string()),
            ..EngineArgs::default()
        };

        let store = crate::engines::pending_work::store_for(&cfg);
        let err = run_remove(&cfg, &store, &args, &noninteractive).unwrap_err();

        assert_matches!(err, PendingWorkError::RemoveRequiresFileModel);
        assert_eq!(
            err.to_string(),
            "remove only supports file-model pending-work items."
        );
    }

    #[test]
    fn missing_note_returns_typed_error_with_legacy_display() {
        let (stage, cfg) = stage_file_item("- [ ] [[GLP-0001]]\n");
        let missing = stage.path().join("notes/glep-shimeji/GLP-0001.md");
        std::fs::remove_file(&missing).unwrap();
        let args = EngineArgs {
            id: Some("GLP-0001".to_string()),
            ..EngineArgs::default()
        };

        let store = crate::engines::pending_work::store_for(&cfg);
        let err = run_remove(&cfg, &store, &args, &noninteractive).unwrap_err();

        assert_matches!(
            err,
            PendingWorkError::WorkItemNoteMissing { ref path } if path == &missing
        );
        assert_eq!(
            err.to_string(),
            format!("Work-item note missing: {}", missing.display())
        );
    }

    fn args_for(id: &str) -> EngineArgs {
        EngineArgs {
            id: Some(id.to_string()),
            ..EngineArgs::default()
        }
    }

    #[test]
    fn interactive_decline_aborts_without_deleting_or_unlinking() {
        let (stage, cfg) = stage_file_item("- [ ] [[GLP-0001]]\n");
        let note = stage.path().join("notes/glep-shimeji/GLP-0001.md");
        let index = stage.path().join("notes/glep-shimeji/glep-shimeji.md");
        let store = crate::engines::pending_work::store_for(&cfg);
        let out = run_remove(&cfg, &store, &args_for("GLP-0001"), &declined)
            .unwrap()
            .into_raw_text();

        assert!(out.contains("aborted"), "got: {out}");
        assert!(out.contains("nothing deleted"), "got: {out}");
        assert!(
            note.exists(),
            "declined removal must leave the note in place"
        );
        assert_eq!(
            std::fs::read_to_string(&index).unwrap(),
            "---\nid: glp\ntitle: glep-shimeji\n---\n\n- [ ] [[GLP-0001]]\n",
            "declined removal must leave the index link in place"
        );
    }

    #[test]
    fn interactive_accept_deletes_note_and_unlinks() {
        let (stage, cfg) = stage_file_item("- [ ] [[GLP-0001]]\n");
        let note = stage.path().join("notes/glep-shimeji/GLP-0001.md");
        let index = stage.path().join("notes/glep-shimeji/glep-shimeji.md");
        let store = crate::engines::pending_work::store_for(&cfg);
        let out = run_remove(&cfg, &store, &args_for("GLP-0001"), &accepted)
            .unwrap()
            .into_raw_text();

        assert!(out.starts_with("REMOVED PWF TASK [GLP-0001]"), "got: {out}");
        assert!(!note.exists(), "accepted removal must delete the note");
        assert_eq!(
            std::fs::read_to_string(&index).unwrap(),
            "---\nid: glp\ntitle: glep-shimeji\n---\n"
        );
    }

    #[test]
    fn assume_yes_deletes_without_consulting_an_interactive_confirmer() {
        let (stage, cfg) = stage_file_item("- [ ] [[GLP-0001]]\n");
        let note = stage.path().join("notes/glep-shimeji/GLP-0001.md");
        let args = EngineArgs {
            assume_yes: true,
            ..args_for("GLP-0001")
        };

        let store = crate::engines::pending_work::store_for(&cfg);
        let out = run_remove(&cfg, &store, &args, &declined)
            .unwrap()
            .into_raw_text();

        assert!(out.starts_with("REMOVED PWF TASK [GLP-0001]"), "got: {out}");
        assert!(!note.exists());
    }

    #[test]
    fn noninteractive_run_proceeds_without_prompting() {
        let (stage, cfg) = stage_file_item("- [ ] [[GLP-0001]]\n");
        let note = stage.path().join("notes/glep-shimeji/GLP-0001.md");

        let store = crate::engines::pending_work::store_for(&cfg);
        let out = run_remove(&cfg, &store, &args_for("GLP-0001"), &noninteractive)
            .unwrap()
            .into_raw_text();

        assert!(out.starts_with("REMOVED PWF TASK [GLP-0001]"), "got: {out}");
        assert!(!note.exists());
    }

    #[test]
    fn confirmation_question_includes_handoff_field_when_gated() {
        let item = Item {
            id: "GLP-0001".to_string(),
            project: "glep-shimeji".to_string(),
            session: "tray gui".to_string(),
            ..Item::empty()
        };
        let note_path = Path::new("/notes/glep-shimeji/GLP-0001.md");
        let handoff_path = Path::new("/repo/docs/handoffs/2026-01-01-tray-gui.md");

        let question = confirmation_question(&item, note_path, Some(handoff_path));

        assert!(
            question.contains("handoff: /repo/docs/handoffs/2026-01-01-tray-gui.md"),
            "got: {question}"
        );
    }

    #[test]
    fn confirmation_question_omits_handoff_field_when_untagged() {
        let item = Item {
            id: "GLP-0001".to_string(),
            project: "glep-shimeji".to_string(),
            session: "tray gui".to_string(),
            ..Item::empty()
        };
        let note_path = Path::new("/notes/glep-shimeji/GLP-0001.md");

        let question = confirmation_question(&item, note_path, None);

        assert!(!question.contains("handoff:"), "got: {question}");
    }

    fn stage_tagged_item(with_handoff_file: bool) -> (tempfile::TempDir, Config, PathBuf) {
        let stage = tempfile::tempdir().unwrap();
        let notes = stage.path().join("notes");
        let project = notes.join("glep-shimeji");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("GLP-0001.md"),
            "---\nid: GLP-0001\nstatus: active\ntitle: tray gui\nproject: glep-shimeji\ncreated: 2026-01-01\ntags: [handoff]\n---\n\nbody\n",
        )
        .unwrap();
        std::fs::write(
            project.join("glep-shimeji.md"),
            "---\nid: glp\ntitle: glep-shimeji\n---\n\n- [ ] [[GLP-0001]]\n",
        )
        .unwrap();

        let repo = stage.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let handoff_dir = repo.join("docs/handoffs");
        if with_handoff_file {
            std::fs::create_dir_all(&handoff_dir).unwrap();
            std::fs::write(
                handoff_dir.join("2026-01-01-tray-gui.md"),
                "---\nstatus: active\nproject: glep-shimeji\ncreated: 2026-01-01\npw: GLP-0001\n---\n\n# Tray GUI\n",
            )
            .unwrap();
        }

        let cfg = crate::config::from_json(
            &format!(
                r#"{{ "notesDir": "{}", "projects": {{ "glep-shimeji": "{}" }}, "prefixes": {{ "glep-shimeji": "GLP" }} }}"#,
                notes.to_string_lossy().replace('\\', "\\\\"),
                repo.to_string_lossy().replace('\\', "\\\\"),
            ),
            None,
        )
        .unwrap();
        (stage, cfg, handoff_dir.join("2026-01-01-tray-gui.md"))
    }

    fn panic_if_consulted(_: &str, _: DefaultAnswer) -> Confirmation {
        panic!("confirmation must not be requested when the mirror preflight already errored");
    }

    #[test]
    fn tagged_item_with_no_handoff_file_errors_before_confirm_prompt() {
        let (stage, cfg, _handoff_path) = stage_tagged_item(false);
        let note = stage.path().join("notes/glep-shimeji/GLP-0001.md");

        let store = crate::engines::pending_work::store_for(&cfg);
        let err = run_remove(&cfg, &store, &args_for("GLP-0001"), &panic_if_consulted).unwrap_err();

        assert_matches!(err, PendingWorkError::HandoffMirror(_));
        assert!(
            note.exists(),
            "note must survive a preflight failure, proving no mutation ran"
        );
    }

    #[test]
    fn interactive_decline_on_tagged_item_leaves_note_and_handoff_untouched() {
        let (stage, cfg, handoff_path) = stage_tagged_item(true);
        let note = stage.path().join("notes/glep-shimeji/GLP-0001.md");
        let store = crate::engines::pending_work::store_for(&cfg);
        let out = run_remove(&cfg, &store, &args_for("GLP-0001"), &declined)
            .unwrap()
            .into_raw_text();

        assert!(out.contains("aborted"), "got: {out}");
        assert!(note.exists(), "declined removal must leave the note");
        assert!(
            handoff_path.exists(),
            "declined removal must leave the handoff file"
        );
    }

    #[test]
    fn interactive_accept_on_tagged_item_deletes_note_and_handoff_and_rebuilds_ledger() {
        let (stage, cfg, handoff_path) = stage_tagged_item(true);
        let note = stage.path().join("notes/glep-shimeji/GLP-0001.md");
        let ledger = stage.path().join("repo/docs/handoffs/LEDGER.md");
        std::fs::write(&ledger, "# stale\n").unwrap();
        let store = crate::engines::pending_work::store_for(&cfg);
        let out = run_remove(&cfg, &store, &args_for("GLP-0001"), &accepted)
            .unwrap()
            .into_raw_text();

        assert!(out.starts_with("REMOVED PWF TASK [GLP-0001]"), "got: {out}");
        assert!(!note.exists(), "accepted removal must delete the note");
        assert!(
            !handoff_path.exists(),
            "accepted removal must delete the linked handoff"
        );
        let ledger_text = std::fs::read_to_string(&ledger).unwrap();
        assert!(
            !ledger_text.contains("GLP-0001"),
            "deleted item should have no ledger row: {ledger_text}"
        );
    }
}
