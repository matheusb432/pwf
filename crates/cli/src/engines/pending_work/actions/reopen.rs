use pwf_application::{
    AppDbStore, IndexEntry, PendingWorkItem,
    pending_work::reopen::{ReopenPendingWork, ReopenPendingWorkError},
};

use super::{
    super::{errors::PendingWorkError, run::require_id},
    close_render::render_reopened,
    done::{map_store_error, mirror_commit_and_append},
};
use crate::{cli::EngineArgs, config::Config, engines::handoff::mirror};

pub(in crate::engines::pending_work) fn run_reopen<S>(
    cfg: &Config,
    store: &S,
    args: &EngineArgs,
) -> Result<String, PendingWorkError>
where
    S: AppDbStore<PendingWorkItem> + AppDbStore<IndexEntry>,
{
    let id = require_id(args, "reopen")?;
    let gate = mirror::handoff_gate(
        cfg,
        store,
        &crate::engines::pending_work::run::project_registry(cfg),
        id,
    )?;
    // An already-active linked pair is an idempotent skip (FR-0021).
    let pending = gate
        .as_ref()
        .map(mirror::preflight_reopen)
        .transpose()?
        .flatten();

    let outcome = pwf_application::pending_work::reopen::execute(
        &ReopenPendingWork { id: id.to_string() },
        store,
        &crate::engines::pending_work::run::project_registry(cfg),
    )
    .map_err(map_reopen_error)?;
    mirror_commit_and_append(render_reopened(&outcome), gate, pending, "reopened")
}

fn map_reopen_error(error: ReopenPendingWorkError) -> PendingWorkError {
    match error {
        ReopenPendingWorkError::ItemNotFound { id } => PendingWorkError::ItemNotFound { id },
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

    fn args(id: &str) -> EngineArgs {
        EngineArgs {
            id: Some(id.to_string()),
            ..EngineArgs::default()
        }
    }

    #[test]
    fn reopen_flips_status_drops_provenance_and_restores_index() {
        let (stage, cfg) = stage_done_item();
        let project = stage.path().join("notes/glep-shimeji");

        let store = crate::engines::pending_work::store_for(&cfg);
        let out = run_reopen(&cfg, &store, &args("GLP-0001")).unwrap();

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

        let store = crate::engines::pending_work::store_for(&cfg);
        let out = run_reopen(&cfg, &store, &args("glp-0001")).unwrap();

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

        let store = crate::engines::pending_work::store_for(&cfg);
        let out = run_reopen(&cfg, &store, &args("GLP-0001")).unwrap();

        assert!(out.contains("already active"), "got: {out}");
    }

    #[test]
    fn reopen_unknown_id_errors() {
        let (_stage, cfg) = stage_done_item();
        let store = crate::engines::pending_work::store_for(&cfg);
        let err = run_reopen(&cfg, &store, &args("GLP-9999")).unwrap_err();
        assert!(matches!(err, PendingWorkError::ItemNotFound { .. }));
    }

    #[test]
    fn reopen_missing_id_errors() {
        let (_stage, cfg) = stage_done_item();
        let store = crate::engines::pending_work::store_for(&cfg);
        let err = run_reopen(&cfg, &store, &EngineArgs::default()).unwrap_err();
        assert!(matches!(err, PendingWorkError::MissingId { action } if action == "reopen"));
    }
}
