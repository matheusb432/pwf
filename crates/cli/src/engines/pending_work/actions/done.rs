// Action: done/cancel status transitions.

use pwf_application::{
    pending_work::{
        cancel::{CancelPendingWork, CancelPendingWorkError},
        done::{CompletePendingWork, CompletePendingWorkError},
    },
    ports::StatusTransitionOutput,
};
use pwf_infra::obsidian::{ObsidianPendingWorkStore, ObsidianPendingWorkStoreError};

use super::{
    super::{errors::PendingWorkError, naming::stamp_date},
    add::{emit_created_section_diagnostic, emit_created_section_diagnostic_for_error},
};
use crate::{
    cli::Args,
    config::Config,
    engines::handoff::mirror::{self, GateItem, MirrorClose, PendingMove},
};

pub(in crate::engines::pending_work) fn run_done(
    cfg: &Config,
    args: &Args,
) -> Result<String, PendingWorkError> {
    let id = args
        .id
        .as_deref()
        .ok_or(PendingWorkError::MissingId { action: "done" })?;
    let completed = stamp_date(args.date.as_deref());
    let (gate, pending) = mirror_close_preflight(
        cfg,
        id,
        MirrorClose::Done,
        &completed,
        args.report.as_deref(),
    )?;
    let store = ObsidianPendingWorkStore::new(cfg.clone());
    let output = pwf_application::pending_work::done::execute(
        CompletePendingWork {
            id: id.to_string(),
            completed,
            report: args.report.clone(),
            commits: args.commits.clone(),
            review: args.review,
        },
        &store,
    )
    .map_err(map_complete_error)?;
    if let Some(review) = output.review_item.as_ref() {
        emit_created_section_diagnostic(review);
    }
    emit_status_diagnostics(&output);
    mirror_commit_and_append(output.text, gate, pending, "archived")
}

pub(in crate::engines::pending_work) fn run_cancel(
    cfg: &Config,
    args: &Args,
) -> Result<String, PendingWorkError> {
    let id = args
        .id
        .as_deref()
        .ok_or(PendingWorkError::MissingId { action: "cancel" })?;
    let Some(report) = args.report.clone() else {
        return Err(PendingWorkError::MissingCancelReport);
    };
    let completed = stamp_date(args.date.as_deref());
    let (gate, pending) = mirror_close_preflight(
        cfg,
        id,
        MirrorClose::Cancelled,
        &completed,
        Some(report.as_str()),
    )?;
    let command = CancelPendingWork::new(
        id.to_string(),
        completed,
        report,
        args.commits.clone(),
        args.review,
    )
    .map_err(map_cancel_error)?;
    let store = ObsidianPendingWorkStore::new(cfg.clone());
    let output = pwf_application::pending_work::cancel::execute(command, &store)
        .map_err(map_cancel_error)?;
    if let Some(review) = output.review_item.as_ref() {
        emit_created_section_diagnostic(review);
    }
    emit_status_diagnostics(&output);
    mirror_commit_and_append(output.text, gate, pending, "archived")
}

/// Gate `id` onto its handoff (PWF-0117) and, when tagged, preflight the
/// archive move — entirely before any pw-item mutation, so a preflight
/// failure (untagged handoff, missing/ambiguous link, archive conflict)
/// leaves the item untouched. Shared by `run_done`/`run_cancel` so the
/// gate+preflight sequence exists exactly once.
fn mirror_close_preflight(
    cfg: &Config,
    id: &str,
    close: MirrorClose,
    date: &str,
    report: Option<&str>,
) -> Result<(Option<GateItem>, Option<PendingMove>), PendingWorkError> {
    let gate = mirror::handoff_gate(cfg, id)?;
    // Inner None = the linked handoff is already archived (e.g. re-running
    // `done` after it already succeeded): nothing to mirror, so the pw
    // handler's own already-closed error is what should surface.
    let pending = gate
        .as_ref()
        .map(|g| mirror::preflight_close(g, close, date, report))
        .transpose()?
        .flatten();
    Ok((gate, pending))
}

/// Commit a preflighted mirror move and append its confirmation line to
/// `text`, or return `text` unchanged when `id` wasn't handoff-tagged.
/// Shared by `run_done`/`run_cancel` (label `"archived"`) and `run_reopen`
/// (label `"reopened"`) — only the label differs between close and reopen.
pub(super) fn mirror_commit_and_append(
    text: String,
    gate: Option<GateItem>,
    pending: Option<PendingMove>,
    label: &str,
) -> Result<String, PendingWorkError> {
    match (gate, pending) {
        (Some(g), Some(p)) => {
            let dest = p.commit().map_err(|source| {
                let remedy = super::super::errors::done_cancel_reopen_remedy(&g.id);
                PendingWorkError::HandoffMirrorAfterMutation {
                    id: g.id,
                    source,
                    remedy,
                }
            })?;
            Ok(format!("{text}\n  handoff: {label} {}", dest.display()))
        }
        _ => Ok(text),
    }
}

fn emit_status_diagnostics(output: &StatusTransitionOutput) {
    if let Some(project) = output.diagnostics.futuro_renamed_project.as_deref() {
        eprintln!("info: normalized `## Futuro` header to `## Future` in {project}");
    }
    if !output.diagnostics.evicted_ids.is_empty() {
        eprintln!(
            "info: archived {} done item(s) past the section cap: {}",
            output.diagnostics.evicted_ids.len(),
            output.diagnostics.evicted_ids.join(", ")
        );
    }
}

fn map_complete_error(error: CompletePendingWorkError) -> PendingWorkError {
    match error {
        CompletePendingWorkError::WriteStore(source) => map_store_error(source.as_ref())
            .unwrap_or_else(|| PendingWorkError::ApplicationWrite(source.to_string())),
        CompletePendingWorkError::ReviewTask(source) => {
            emit_created_section_diagnostic_for_error(&source);
            PendingWorkError::ApplicationWrite(source.to_string())
        }
    }
}

fn map_cancel_error(error: CancelPendingWorkError) -> PendingWorkError {
    match error {
        CancelPendingWorkError::EmptyReport => PendingWorkError::EmptyReport,
        CancelPendingWorkError::WriteStore(source) => map_store_error(source.as_ref())
            .unwrap_or_else(|| PendingWorkError::ApplicationWrite(source.to_string())),
        CancelPendingWorkError::ReviewTask(source) => {
            emit_created_section_diagnostic_for_error(&source);
            PendingWorkError::ApplicationWrite(source.to_string())
        }
    }
}

pub(super) fn map_store_error(
    source: &(dyn std::error::Error + Send + Sync + 'static),
) -> Option<PendingWorkError> {
    let error = source.downcast_ref::<ObsidianPendingWorkStoreError>()?;
    match error {
        ObsidianPendingWorkStoreError::ItemNotFound { id } => {
            Some(PendingWorkError::ItemNotFound { id: id.clone() })
        }
        ObsidianPendingWorkStoreError::AmbiguousId { id } => {
            Some(PendingWorkError::AmbiguousId { id: id.clone() })
        }
        ObsidianPendingWorkStoreError::NotesDirectoryNotFound { path } => {
            Some(PendingWorkError::NotesDirectoryNotFound { path: path.clone() })
        }
        ObsidianPendingWorkStoreError::WorkItemNoteMissing { path } => {
            Some(PendingWorkError::WorkItemNoteMissing { path: path.clone() })
        }
        ObsidianPendingWorkStoreError::EmptyReport => Some(PendingWorkError::EmptyReport),
        ObsidianPendingWorkStoreError::ExpectedOpenTaskMarker { note, line } => {
            Some(PendingWorkError::ExpectedOpenTaskMarker {
                note: note.clone(),
                line: *line,
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::{assert_matches, path::Path};

    use super::*;
    use crate::engines::pending_work::errors::PendingWorkError;

    fn review_add_write_index_error() -> pwf_application::pending_work::add::AddPendingWorkError {
        pwf_application::pending_work::add::AddPendingWorkError::WriteStore(Box::new(
            ObsidianPendingWorkStoreError::AddWriteIndexFile {
                source: std::io::Error::other("index write failed"),
                project: "glep-shimeji".to_string(),
                created_section: Some("Human".to_string()),
            },
        ))
    }

    fn stage_tagged_handoff_item() -> (tempfile::TempDir, Config, std::path::PathBuf) {
        let (stage, cfg) = stage_file_item();
        let item_path = stage.path().join("notes/glep-shimeji/GLP-0001.md");
        std::fs::write(
            &item_path,
            "---\nid: GLP-0001\nstatus: active\ntitle: tray gui\nproject: glep-shimeji\ncreated: 2026-01-01\ntags: [handoff]\n---\n\nbody\n",
        )
        .unwrap();
        (stage, cfg, item_path)
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

    fn stage_file_item() -> (tempfile::TempDir, Config) {
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
            "---\nid: glp\ntitle: glep-shimeji\n---\n\n- [ ] [[GLP-0001]]\n",
        )
        .unwrap();
        let cfg = cfg(&notes);
        (stage, cfg)
    }

    #[test]
    fn missing_id_returns_typed_error_with_legacy_display() {
        let (_stage, cfg) = stage_file_item();

        let err = run_done(&cfg, &Args::default()).unwrap_err();

        assert_matches!(
            err,
            PendingWorkError::MissingId { action } if action == "done"
        );
        assert_eq!(err.to_string(), "--id is required for done.");
    }

    #[test]
    fn complete_review_error_exposes_created_section_before_cli_mapping() {
        let error = CompletePendingWorkError::ReviewTask(review_add_write_index_error());
        let CompletePendingWorkError::ReviewTask(source) = error else {
            panic!("expected review-task error");
        };

        assert_eq!(
            super::super::add::created_section_diagnostic_for_error(&source),
            Some(("glep-shimeji", "Human"))
        );
        assert_matches!(
            map_complete_error(CompletePendingWorkError::ReviewTask(source)),
            PendingWorkError::ApplicationWrite(message) if message == "Failed to write index file: index write failed"
        );
    }

    #[test]
    fn cancel_review_error_exposes_created_section_before_cli_mapping() {
        let error = CancelPendingWorkError::ReviewTask(review_add_write_index_error());
        let CancelPendingWorkError::ReviewTask(source) = error else {
            panic!("expected review-task error");
        };

        assert_eq!(
            super::super::add::created_section_diagnostic_for_error(&source),
            Some(("glep-shimeji", "Human"))
        );
        assert_matches!(
            map_cancel_error(CancelPendingWorkError::ReviewTask(source)),
            PendingWorkError::ApplicationWrite(message) if message == "Failed to write index file: index write failed"
        );
    }

    #[test]
    fn empty_report_returns_typed_error_with_legacy_display() {
        let (_stage, cfg) = stage_file_item();
        let args = Args {
            id: Some("GLP-0001".to_string()),
            report: Some(" \t\n".to_string()),
            ..Args::default()
        };

        let err = run_done(&cfg, &args).unwrap_err();

        assert_matches!(err, PendingWorkError::EmptyReport);
        assert_eq!(err.to_string(), "--report cannot be empty.");
    }

    #[test]
    fn done_with_review_appends_review_task_as_text() {
        // --review should append the added task as text, never JSON (PWF-0059).
        let (_stage, cfg) = stage_file_item();
        let args = Args {
            id: Some("GLP-0001".to_string()),
            review: true,
            date: Some("2026-01-01".to_string()),
            ..Args::default()
        };

        let out = run_done(&cfg, &args).unwrap();

        assert!(out.starts_with("Done GLP-0001"), "got: {out}");
        assert!(out.contains("ADDED PWF TASK ["), "got: {out}");
    }

    #[test]
    fn done_tagged_item_errors_before_mutation_when_repo_root_missing() {
        let (_stage, cfg, item_path) = stage_tagged_handoff_item();
        let args = Args {
            id: Some("GLP-0001".to_string()),
            date: Some("2026-01-01".to_string()),
            ..Args::default()
        };

        let err = run_done(&cfg, &args).unwrap_err();

        assert_matches!(err, PendingWorkError::HandoffMirror(_));
        let item = std::fs::read_to_string(&item_path).unwrap();
        assert!(
            item.contains("status: active"),
            "item mutated despite preflight failure: {item}"
        );
    }

    #[test]
    fn cancel_tagged_item_errors_before_mutation_when_repo_root_missing() {
        let (_stage, cfg, item_path) = stage_tagged_handoff_item();
        let args = Args {
            id: Some("GLP-0001".to_string()),
            report: Some("obsoleted".to_string()),
            date: Some("2026-01-01".to_string()),
            ..Args::default()
        };

        let err = run_cancel(&cfg, &args).unwrap_err();

        assert_matches!(err, PendingWorkError::HandoffMirror(_));
        let item = std::fs::read_to_string(&item_path).unwrap();
        assert!(
            item.contains("status: active"),
            "item mutated despite preflight failure: {item}"
        );
    }

    /// Re-running `done` on an already-done tagged item whose handoff was
    /// already archived must surface the pw layer's canonical already-closed
    /// error (`ItemNotFound`), not a mirror `HandoffNotFound` that misdirects
    /// with an "untag it / create the handoff" hint — the handoff is exactly
    /// where a first `done` left it, so there is nothing to fix there.
    #[test]
    fn done_on_already_closed_tagged_item_surfaces_pw_layer_error_not_mirror_hint() {
        let stage = tempfile::tempdir().unwrap();
        let notes = stage.path().join("notes");
        let project = notes.join("glep-shimeji");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            project.join("GLP-0001.md"),
            "---\nid: GLP-0001\nstatus: done\ntitle: tray gui\nproject: glep-shimeji\ncreated: 2026-01-01\ncompleted: 2026-01-02\ntags: [handoff]\n---\n\nbody\n",
        )
        .unwrap();
        std::fs::write(
            project.join("glep-shimeji.md"),
            "---\nid: glp\ntitle: glep-shimeji\n---\n\n- [x] [[GLP-0001]] \u{2705} 2026-01-02\n",
        )
        .unwrap();

        let repo = stage.path().join("repo");
        let archived_dir = repo.join("docs/handoffs/archived");
        std::fs::create_dir_all(&archived_dir).unwrap();
        let handoff_path = archived_dir.join("2026-01-01-tray-gui.md");
        std::fs::write(
            &handoff_path,
            "---\nstatus: done\ncompleted: 2026-01-02\nproject: glep-shimeji\ncreated: 2026-01-01\npw: GLP-0001\n---\n\n# Tray GUI\n",
        )
        .unwrap();
        let handoff_before = std::fs::read_to_string(&handoff_path).unwrap();

        let cfg = crate::config::from_json(
            &format!(
                r#"{{ "notesDir": "{}", "projects": {{ "glep-shimeji": "{}" }}, "prefixes": {{ "glep-shimeji": "GLP" }} }}"#,
                notes.to_string_lossy().replace('\\', "\\\\"),
                repo.to_string_lossy().replace('\\', "\\\\"),
            ),
            None,
        )
        .unwrap();
        let args = Args {
            id: Some("GLP-0001".to_string()),
            date: Some("2026-01-03".to_string()),
            ..Args::default()
        };

        let err = run_done(&cfg, &args).unwrap_err();

        assert_matches!(err, PendingWorkError::ItemNotFound { ref id } if id == "GLP-0001");
        let message = err.to_string();
        assert!(
            !message.contains("untag") && !message.contains("create the handoff"),
            "error should not carry the mirror's untag/create hint: {message}"
        );
        assert_eq!(
            std::fs::read_to_string(&handoff_path).unwrap(),
            handoff_before,
            "archived handoff must be untouched by a no-op preflight skip"
        );
    }
}
