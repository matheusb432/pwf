// Action: done/cancel status transitions.

use cqrsy::Sender;
use pwf_application::{
    AddPendingWorkError, AddPendingWorkItem, AddPendingWorkItemHandler, CancelPendingWork,
    CancelPendingWorkError, CancelPendingWorkHandler, CompletePendingWork,
    CompletePendingWorkError, CompletePendingWorkHandler, StatusTransitionOutput,
};
use pwf_domain::pending_work::AddedItem;
use pwf_infra::obsidian::{ObsidianPendingWorkStore, ObsidianPendingWorkStoreError};

use super::{
    super::{errors::PendingWorkError, naming::stamp_date},
    add::{emit_created_section_diagnostic, emit_created_section_diagnostic_for_error},
};
use crate::{cli::Args, config::Config};

pub(in crate::engines::pending_work) fn run_done(
    cfg: &Config,
    args: &Args,
) -> Result<String, PendingWorkError> {
    let id = args
        .id
        .as_deref()
        .ok_or(PendingWorkError::MissingId { action: "done" })?;
    let store = ObsidianPendingWorkStore::new(cfg.clone());
    let add_sender = ReviewAddSender::new(store.clone());
    let handler = CompletePendingWorkHandler::new(store, add_sender);
    let output = cqrsy::send_now(
        &(),
        &handler,
        CompletePendingWork {
            id: id.to_string(),
            completed: stamp_date(args.date.as_deref()),
            report: args.report.clone(),
            commits: args.commits.clone(),
            review: args.review,
        },
    )
    .map_err(map_complete_error)?;
    emit_status_diagnostics(&output);
    Ok(output.text)
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
    let command = CancelPendingWork::new(
        id.to_string(),
        stamp_date(args.date.as_deref()),
        report,
        args.commits.clone(),
        args.review,
    )
    .map_err(map_cancel_error)?;
    let store = ObsidianPendingWorkStore::new(cfg.clone());
    let add_sender = ReviewAddSender::new(store.clone());
    let handler = CancelPendingWorkHandler::new(store, add_sender);
    let output = cqrsy::send_now(&(), &handler, command).map_err(map_cancel_error)?;
    emit_status_diagnostics(&output);
    Ok(output.text)
}

#[derive(Clone)]
struct ReviewAddSender {
    handler: AddPendingWorkItemHandler<ObsidianPendingWorkStore>,
}

impl ReviewAddSender {
    fn new(store: ObsidianPendingWorkStore) -> Self {
        Self {
            handler: AddPendingWorkItemHandler::new(store),
        }
    }
}

impl Sender<AddPendingWorkItem> for ReviewAddSender {
    async fn send(&self, req: AddPendingWorkItem) -> Result<AddedItem, AddPendingWorkError> {
        let result = cqrsy::send(&(), &self.handler, req).await;
        match &result {
            Ok(added) => emit_created_section_diagnostic(added),
            Err(error) => emit_created_section_diagnostic_for_error(error),
        }
        result
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
            "---\nstatus: active\ntitle: tray gui\nproject: glep-shimeji\ncreated: 2026-01-01\n---\n\nbody\n",
        )
        .unwrap();
        std::fs::write(project.join("glep-shimeji.md"), "- [ ] [[GLP-0001]]\n").unwrap();
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
}
