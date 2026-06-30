// Action: reopen (the inverse of check/cancel).
//
// Flips a closed item's status back to `active`, drops its `completed:`/`commits:`
// provenance, moves an evicted note out of `_archive` back into the project dir,
// and restores its index link (flipping the done-queue entry back to an open
// `- [ ] [[ID]]`, or re-adding the link when it was evicted past the cap).

use super::super::{
    done_queue,
    errors::PendingWorkError,
    index::{add_link_to_index, reopen_status_text, set_commits_text},
    naming::{project_dir, project_index_path},
    obsidian::store::ObsidianStore,
    query::find_item_note_with_project,
    run::require_id,
};
use crate::{cli::Args, config::Config};

pub(in crate::engines::pending_work) fn run_reopen(
    cfg: &Config,
    args: &Args,
) -> Result<String, PendingWorkError> {
    let id = require_id(args, "reopen")?;
    let (project, note_path) = find_item_note_with_project(cfg, id)
        .ok_or_else(|| PendingWorkError::ItemNotFound { id: id.to_string() })?;

    // Canonical id from the note filename, so output + the restored index link use
    // the on-disk casing regardless of how the caller typed `--id`.
    let canonical = note_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(id)
        .to_string();

    let content = ObsidianStore::read_item_file(&note_path)?;
    // Idempotent: an already-active item is the goal state — skip without mutating,
    // mirroring `handoff done`'s already-closed handling.
    if crate::frontmatter::parse(&content)
        .frontmatter
        .get("status")
        .map(String::as_str)
        == Some("active")
    {
        return Ok(format!(
            "{canonical} already active ({project}) — skipped\n"
        ));
    }

    // Frontmatter: status -> active, drop completed + commits provenance.
    let updated = set_commits_text(&reopen_status_text(&content), None);

    // Move the note back into the project dir when it was evicted to `_archive`.
    let notes_dir = cfg.notes_dir_for(&project);
    let dir = project_dir(notes_dir, &project);
    let target = dir.join(format!("{canonical}.md"));
    ObsidianStore::write_item_file(&target, &updated)?;
    if target != note_path {
        ObsidianStore::remove_item_file(&note_path)?;
    }

    // Restore the index link: flip the done-queue entry back to open in place, or
    // re-add an open link when the item had been evicted past the section cap.
    let index_path = project_index_path(notes_dir, &project);
    if index_path.exists() {
        let idx = ObsidianStore::read_index(&index_path)?;
        let restored = match done_queue::reopen_done_link(&idx, &canonical) {
            Some(flipped) => flipped,
            None => add_link_to_index(&idx, &format!("- [ ] [[{canonical}]]")),
        };
        ObsidianStore::write_index(&index_path, &restored)?;
    }

    Ok(format!("Reopened {canonical} ({project})\n"))
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

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

    fn nanos() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }

    /// Stage a project with one done item still in the project dir, marked done in
    /// the index done-queue.
    fn stage_done_item() -> (PathBuf, Config) {
        let stage = std::env::temp_dir().join(format!("pwf_reopen_{}", nanos()));
        let notes = stage.join("notes");
        let project = notes.join("glep-shimeji");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("GLP-0001.md"),
            "---\nstatus: done\ntitle: tray gui\nproject: glep-shimeji\ncreated: 2026-01-01\ncompleted: 2026-01-02\ncommits: \"a..b\"\n---\n\nbody\n",
        )
        .unwrap();
        std::fs::write(
            project.join("glep-shimeji.md"),
            "- [x] [[GLP-0001]] ✅ 2026-01-02\n",
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
        let project = stage.join("notes/glep-shimeji");

        let out = run_reopen(&cfg, &args("GLP-0001")).unwrap();

        assert!(out.starts_with("Reopened GLP-0001"), "got: {out}");
        let note = std::fs::read_to_string(project.join("GLP-0001.md")).unwrap();
        assert!(note.contains("status: active"), "status: {note}");
        assert!(!note.contains("completed:"), "completed lingered: {note}");
        assert!(!note.contains("commits:"), "commits lingered: {note}");
        let index = std::fs::read_to_string(project.join("glep-shimeji.md")).unwrap();
        assert_eq!(index, "- [ ] [[GLP-0001]]\n", "index not restored: {index}");
    }

    #[test]
    fn reopen_moves_archived_note_back_to_project_dir() {
        let (stage, cfg) = stage_done_item();
        let project = stage.join("notes/glep-shimeji");
        let archive = project.join("_archive");
        std::fs::create_dir_all(&archive).unwrap();
        // Simulate an eviction: note lives in _archive, index link is gone.
        std::fs::rename(project.join("GLP-0001.md"), archive.join("GLP-0001.md")).unwrap();
        std::fs::write(project.join("glep-shimeji.md"), "# glep-shimeji\n").unwrap();

        let out = run_reopen(&cfg, &args("glp-0001")).unwrap();

        assert!(out.starts_with("Reopened GLP-0001"), "got: {out}");
        assert!(
            project.join("GLP-0001.md").exists(),
            "note not moved back to project dir"
        );
        assert!(
            !archive.join("GLP-0001.md").exists(),
            "note still in _archive"
        );
        let index = std::fs::read_to_string(project.join("glep-shimeji.md")).unwrap();
        assert!(
            index.contains("- [ ] [[GLP-0001]]"),
            "evicted link not re-added: {index}"
        );
    }

    #[test]
    fn reopen_already_active_item_skips() {
        let (stage, cfg) = stage_done_item();
        let project = stage.join("notes/glep-shimeji");
        std::fs::write(
            project.join("GLP-0001.md"),
            "---\nstatus: active\ntitle: tray gui\nproject: glep-shimeji\ncreated: 2026-01-01\n---\n\nbody\n",
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
