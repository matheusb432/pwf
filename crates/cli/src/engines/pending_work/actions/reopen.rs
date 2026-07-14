// Action: reopen (the inverse of done/cancel).

use pwf_application::pending_work::reopen::{ReopenPendingWork, ReopenPendingWorkError};
use pwf_infra::obsidian::ObsidianPendingWorkStore;

use super::{
    super::{errors::PendingWorkError, run::require_id},
    done::{map_store_error, mirror_commit_and_append},
};
use crate::{cli::Args, config::Config, engines::handoff::mirror};

pub(in crate::engines::pending_work) fn run_reopen(
    cfg: &Config,
    args: &Args,
) -> Result<String, PendingWorkError> {
    let id = require_id(args, "reopen")?;
    let gate = mirror::handoff_gate(cfg, id)?;
    // Inner None = the pair is already active (idempotent skip, FR-0021).
    let pending = gate
        .as_ref()
        .map(mirror::preflight_reopen)
        .transpose()?
        .flatten();

    let store = ObsidianPendingWorkStore::new(cfg.clone());
    let text = pwf_application::pending_work::reopen::execute(
        ReopenPendingWork { id: id.to_string() },
        &store,
    )
    .map_err(map_reopen_error)?;
    mirror_commit_and_append(text, gate, pending, "reopened")
}

fn map_reopen_error(error: ReopenPendingWorkError) -> PendingWorkError {
    match error {
        ReopenPendingWorkError::WriteStore(source) => map_store_error(source.as_ref())
            .unwrap_or_else(|| PendingWorkError::ApplicationWrite(source.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

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

    /// Stage a project with one done item still in the project dir, marked done in
    /// the index done-queue.
    fn stage_done_item() -> (tempfile::TempDir, Config) {
        let stage = tempfile::tempdir().unwrap();
        let notes = stage.path().join("notes");
        let project = notes.join("glep-shimeji");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("GLP-0001.md"),
            "---\nid: GLP-0001\nstatus: done\ntitle: tray gui\nproject: glep-shimeji\ncreated: 2026-01-01\ncompleted: 2026-01-02\ncommits: \"a..b\"\n---\n\nbody\n",
        )
        .unwrap();
        std::fs::write(
            project.join("glep-shimeji.md"),
            "---\nid: glp\ntitle: glep-shimeji\n---\n\n- [x] [[GLP-0001]] ✅ 2026-01-02\n",
        )
        .unwrap();
        let cfg = cfg(&notes);
        (stage, cfg)
    }

    fn args(id: &str) -> Args {
        Args {
            id: Some(id.to_string()),
            ..Args::default()
        }
    }

    #[test]
    fn reopen_flips_status_drops_provenance_and_restores_index() {
        let (stage, cfg) = stage_done_item();
        let project = stage.path().join("notes/glep-shimeji");

        let out = run_reopen(&cfg, &args("GLP-0001")).unwrap();

        assert!(out.starts_with("Reopened GLP-0001"), "got: {out}");
        let note = std::fs::read_to_string(project.join("GLP-0001.md")).unwrap();
        assert!(note.contains("status: active"), "status: {note}");
        assert!(!note.contains("completed:"), "completed lingered: {note}");
        assert!(!note.contains("commits:"), "commits lingered: {note}");
        let index = std::fs::read_to_string(project.join("glep-shimeji.md")).unwrap();
        assert_eq!(
            index, "---\nid: glp\ntitle: glep-shimeji\n---\n\n- [ ] [[GLP-0001]]\n",
            "index not restored: {index}"
        );
    }

    #[test]
    fn reopen_readds_evicted_link_without_moving_note() {
        let (stage, cfg) = stage_done_item();
        let project = stage.path().join("notes/glep-shimeji");
        std::fs::write(
            project.join("glep-shimeji.md"),
            "---\nid: glp\ntitle: glep-shimeji\n---\n\n# glep-shimeji\n",
        )
        .unwrap();

        let out = run_reopen(&cfg, &args("glp-0001")).unwrap();

        assert!(out.starts_with("Reopened GLP-0001"), "got: {out}");
        assert!(
            project.join("GLP-0001.md").exists(),
            "note must remain in project dir"
        );
        assert!(!project.join("_archive").exists());
        let index = std::fs::read_to_string(project.join("glep-shimeji.md")).unwrap();
        assert!(
            index.contains("- [ ] [[GLP-0001]]"),
            "evicted link not re-added: {index}"
        );
    }

    #[test]
    fn reopen_already_active_item_skips() {
        let (stage, cfg) = stage_done_item();
        let project = stage.path().join("notes/glep-shimeji");
        std::fs::write(
            project.join("GLP-0001.md"),
            "---\nid: GLP-0001\nstatus: active\ntitle: tray gui\nproject: glep-shimeji\ncreated: 2026-01-01\n---\n\nbody\n",
        )
        .unwrap();

        let out = run_reopen(&cfg, &args("GLP-0001")).unwrap();

        assert!(out.contains("already active"), "got: {out}");
    }

    #[test]
    fn reopen_unknown_id_errors() {
        let (_stage, cfg) = stage_done_item();
        let err = run_reopen(&cfg, &args("GLP-9999")).unwrap_err();
        assert!(matches!(err, PendingWorkError::ItemNotFound { .. }));
    }

    #[test]
    fn reopen_missing_id_errors() {
        let (_stage, cfg) = stage_done_item();
        let err = run_reopen(&cfg, &Args::default()).unwrap_err();
        assert!(matches!(err, PendingWorkError::MissingId { action } if action == "reopen"));
    }
}
