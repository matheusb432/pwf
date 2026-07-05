// Action: remove.

use std::path::Path;

use super::super::{
    errors::PendingWorkError, index::remove_index_link, model::Item, naming::project_index_path,
    obsidian::store::ObsidianStore, query::find_pending_item,
};
use crate::{
    cli::Args,
    config::Config,
    confirm::{Confirm, DefaultAnswer},
    confirm_prompt::{ConfirmationPrompt, Field},
};

fn remove_link_from_index_content(
    index_content: &str,
    id: &str,
) -> Result<String, PendingWorkError> {
    let removed = remove_index_link(index_content, id);
    if removed == index_content {
        return Err(PendingWorkError::IndexLinkNotFound { id: id.to_string() });
    }
    Ok(removed)
}

/// Last-look prompt before an irreversible delete: identifies the task and the
/// note file that is about to be unlinked and removed.
fn confirmation_question(item: &Item, item_path: &Path) -> String {
    let fields = [
        Field::new("task_id", item.id.clone()),
        Field::new("project", item.project.clone()),
        Field::new("title", item.session.clone()),
        Field::new("note", item_path.display().to_string()),
    ];
    ConfirmationPrompt::new(
        "Confirm task removal",
        &fields,
        "Remove this pending-work task?",
    )
    .to_string()
}

pub(in crate::engines::pending_work) fn run_remove(
    cfg: &Config,
    args: &Args,
    confirmer: &impl Confirm,
) -> Result<String, PendingWorkError> {
    let id = args
        .id
        .as_deref()
        .ok_or(PendingWorkError::MissingId { action: "remove" })?;

    let item = find_pending_item(cfg, id)?;
    let item_file = item
        .item_file
        .as_deref()
        .ok_or(PendingWorkError::RemoveRequiresFileModel)?;
    let item_path = Path::new(item_file);
    if !item_path.exists() {
        return Err(PendingWorkError::WorkItemNoteMissing {
            path: item_path.to_path_buf(),
        });
    }

    // Default-yes gate: an interactive operator can abort a mistaken delete.
    // `--yes` skips it; a non-interactive caller (agentic dispatch, pipe, CI)
    // proceeds without prompting so scripted removals stay unattended.
    if !args.assume_yes && confirmer.interactive() {
        let question = confirmation_question(&item, item_path);
        if !confirmer.confirm(&question, DefaultAnswer::Yes) {
            return Ok(format!(
                "# remove {} — aborted\nnothing deleted.\n",
                item.id
            ));
        }
    }

    let index_path = project_index_path(cfg.notes_dir_for(&item.project), &item.project);
    let index_content = ObsidianStore::read_index(&index_path)?;
    let removed = remove_link_from_index_content(&index_content, &item.id)?;
    ObsidianStore::write_index(&index_path, &removed)?;
    ObsidianStore::remove_item_file(item_path)?;

    Ok(format!(
        "REMOVED PWF TASK [{}] {} :: {}\n  deleted: {}\n  unlinked: {}\n",
        item.id,
        item.project,
        item.session,
        item_path.display(),
        index_path.display()
    ))
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

    use super::*;
    use crate::{confirm::FakeConfirm, engines::pending_work::errors::PendingWorkError};

    /// Non-interactive confirmer: the removal gate proceeds without prompting,
    /// matching an agentic / piped run.
    const NONINTERACTIVE: FakeConfirm = FakeConfirm {
        interactive: false,
        answer: false,
    };

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
            "---\nstatus: active\ntitle: tray gui\nproject: glep-shimeji\ncreated: 2026-01-01\n---\n\nbody\n",
        )
        .unwrap();
        std::fs::write(project.join("glep-shimeji.md"), index).unwrap();
        let cfg = cfg(&notes);
        (stage, cfg)
    }

    #[test]
    fn missing_id_returns_typed_error_with_legacy_display() {
        let (_stage, cfg) = stage_file_item("- [ ] [[GLP-0001]]\n");

        let err = run_remove(&cfg, &Args::default(), &NONINTERACTIVE).unwrap_err();

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
        std::fs::write(project.join("glep-shimeji.md"), "- [ ] `legacy` :: do it\n").unwrap();
        let cfg = cfg(&notes);
        let args = Args {
            id: Some("glep-shimeji:1".to_string()),
            ..Args::default()
        };

        let err = run_remove(&cfg, &args, &NONINTERACTIVE).unwrap_err();

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
        let args = Args {
            id: Some("GLP-0001".to_string()),
            ..Args::default()
        };

        let err = run_remove(&cfg, &args, &NONINTERACTIVE).unwrap_err();

        assert_matches!(
            err,
            PendingWorkError::WorkItemNoteMissing { ref path } if path == &missing
        );
        assert_eq!(
            err.to_string(),
            format!("Work-item note missing: {}", missing.display())
        );
    }

    #[test]
    fn missing_index_link_returns_typed_error_with_legacy_display() {
        let err = remove_link_from_index_content("- [ ] [[GLP-9999]]\n", "GLP-0001").unwrap_err();

        assert_matches!(
            err,
            PendingWorkError::IndexLinkNotFound { ref id } if id == "GLP-0001"
        );
        assert_eq!(err.to_string(), "Index link not found for GLP-0001.");
    }

    fn args_for(id: &str) -> Args {
        Args {
            id: Some(id.to_string()),
            ..Args::default()
        }
    }

    #[test]
    fn interactive_decline_aborts_without_deleting_or_unlinking() {
        let (stage, cfg) = stage_file_item("- [ ] [[GLP-0001]]\n");
        let note = stage.path().join("notes/glep-shimeji/GLP-0001.md");
        let index = stage.path().join("notes/glep-shimeji/glep-shimeji.md");
        let declines = FakeConfirm {
            interactive: true,
            answer: false,
        };

        let out = run_remove(&cfg, &args_for("GLP-0001"), &declines).unwrap();

        assert!(out.contains("aborted"), "got: {out}");
        assert!(out.contains("nothing deleted"), "got: {out}");
        assert!(
            note.exists(),
            "declined removal must leave the note in place"
        );
        assert_eq!(
            std::fs::read_to_string(&index).unwrap(),
            "- [ ] [[GLP-0001]]\n",
            "declined removal must leave the index link in place"
        );
    }

    #[test]
    fn interactive_accept_deletes_note_and_unlinks() {
        let (stage, cfg) = stage_file_item("- [ ] [[GLP-0001]]\n");
        let note = stage.path().join("notes/glep-shimeji/GLP-0001.md");
        let index = stage.path().join("notes/glep-shimeji/glep-shimeji.md");
        let accepts = FakeConfirm {
            interactive: true,
            answer: true,
        };

        let out = run_remove(&cfg, &args_for("GLP-0001"), &accepts).unwrap();

        assert!(out.starts_with("REMOVED PWF TASK [GLP-0001]"), "got: {out}");
        assert!(!note.exists(), "accepted removal must delete the note");
        assert_eq!(std::fs::read_to_string(&index).unwrap(), "");
    }

    #[test]
    fn assume_yes_deletes_without_consulting_an_interactive_confirmer() {
        let (stage, cfg) = stage_file_item("- [ ] [[GLP-0001]]\n");
        let note = stage.path().join("notes/glep-shimeji/GLP-0001.md");
        // A declining confirmer proves `--yes` never consults it.
        let would_decline = FakeConfirm {
            interactive: true,
            answer: false,
        };
        let args = Args {
            assume_yes: true,
            ..args_for("GLP-0001")
        };

        let out = run_remove(&cfg, &args, &would_decline).unwrap();

        assert!(out.starts_with("REMOVED PWF TASK [GLP-0001]"), "got: {out}");
        assert!(!note.exists());
    }

    #[test]
    fn noninteractive_run_proceeds_without_prompting() {
        let (stage, cfg) = stage_file_item("- [ ] [[GLP-0001]]\n");
        let note = stage.path().join("notes/glep-shimeji/GLP-0001.md");

        let out = run_remove(&cfg, &args_for("GLP-0001"), &NONINTERACTIVE).unwrap();

        assert!(out.starts_with("REMOVED PWF TASK [GLP-0001]"), "got: {out}");
        assert!(!note.exists());
    }
}
