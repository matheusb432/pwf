// Action: check (mark done).

use std::path::Path;

use super::{
    super::{
        commits, done_queue,
        errors::PendingWorkError,
        index::{append_report_text, set_commits_text, set_status_text},
        naming::{project_dir, project_index_path, stamp_date},
        obsidian::store::ObsidianStore,
        query::find_pending_item,
        section::Section,
    },
    add::{NewItemSpec, add_pending_work_item},
};
use crate::{cli::Args, config::Config};

#[derive(Clone, Copy)]
enum CloseAction {
    Check,
    Cancel,
}

impl CloseAction {
    fn verb(self) -> &'static str {
        match self {
            CloseAction::Check => "check",
            CloseAction::Cancel => "cancel",
        }
    }

    fn frontmatter_status(self) -> &'static str {
        match self {
            CloseAction::Check => "done",
            CloseAction::Cancel => "cancelled",
        }
    }

    fn past_tense(self) -> &'static str {
        match self {
            CloseAction::Check => "Checked",
            CloseAction::Cancel => "Cancelled",
        }
    }

    fn requires_report(self) -> bool {
        matches!(self, CloseAction::Cancel)
    }
}

pub(in crate::engines::pending_work) fn run_check(
    cfg: &Config,
    args: &Args,
) -> Result<String, PendingWorkError> {
    run_close(cfg, args, CloseAction::Check)
}

pub(in crate::engines::pending_work) fn run_cancel(
    cfg: &Config,
    args: &Args,
) -> Result<String, PendingWorkError> {
    run_close(cfg, args, CloseAction::Cancel)
}

fn run_close(cfg: &Config, args: &Args, action: CloseAction) -> Result<String, PendingWorkError> {
    let id = args.id.as_deref().ok_or(PendingWorkError::MissingId {
        action: action.verb(),
    })?;
    if action.requires_report() && args.report.is_none() {
        return Err(PendingWorkError::MissingCancelReport);
    }
    let item = find_pending_item(cfg, id)?;
    let date = stamp_date(&args.date);

    if let Some(ref file) = item.item_file {
        // File-model path
        let item_path = Path::new(file);
        let content = ObsidianStore::read_item_file(item_path)?;
        let content = if let Some(report) = args.report.as_deref() {
            append_report_text(&content, report).ok_or(PendingWorkError::EmptyReport)?
        } else {
            content
        };
        // Record commit-range provenance only when supplied so the default check
        // output stays unchanged (PWF-0017).
        let commits_value = commits::frontmatter_value(&args.commits);
        let content = match commits_value.as_deref() {
            Some(v) => set_commits_text(&content, Some(v)),
            None => content,
        };
        let updated = set_status_text(&content, action.frontmatter_status(), &date);
        ObsidianStore::write_item_file(item_path, &updated)?;

        // Keep closed items in the index as a capped, rotating done-queue (PWF-0026).
        let notes_dir = cfg.notes_dir_for(&item.project);
        let index_path = project_index_path(notes_dir, &item.project);
        if index_path.exists() {
            let idx_content = ObsidianStore::read_index(&index_path)?;
            let queue = done_queue::mark_done(&idx_content, id, &date);
            ObsidianStore::write_index(&index_path, &queue.content)?;
            if queue.futuro_renamed {
                eprintln!(
                    "info: normalized `## Futuro` header to `## Future` in {}",
                    item.project
                );
            }
            let dir = project_dir(notes_dir, &item.project);
            for ev in &queue.evicted {
                ObsidianStore::archive_item_file(&dir, ev)?;
            }
            if !queue.evicted.is_empty() {
                eprintln!(
                    "info: archived {} done item(s) past the section cap: {}",
                    queue.evicted.len(),
                    queue.evicted.join(", ")
                );
            }
        }

        // Explicit-only: spawn a `## Human` review task prepped with git-tools diff
        // commands for the recorded range (or the unpushed fallback) (PWF-0017).
        let review = if args.review {
            let prompt = commits::review_task_prompt(&item.id, commits_value.as_deref());
            Some(add_pending_work_item(
                cfg,
                &NewItemSpec {
                    project_name: &item.project,
                    task_prompt: &prompt,
                    task_title: None,
                    created: &date,
                    section: Some(Section::Human),
                    prereq: None,
                    effort: None,
                },
            )?)
        } else {
            None
        };

        let mut out = format!(
            "{} {} ({} :: {})\n",
            action.past_tense(),
            item.id,
            item.project,
            item.session
        );
        if let Some(review) = review {
            out.push_str(&review);
        }
        return Ok(out);
    }

    // Legacy checkbox path
    let note_path = Path::new(&item.note);
    let content = ObsidianStore::read_note(note_path)?;
    // Bounds-checked slice: if the note shrank since it was listed, return the
    // "may have changed" error rather than panicking on the out-of-range slice.
    let marker = content
        .get(item.marker_index..item.marker_index + 5)
        .unwrap_or("");
    if marker != "- [ ]" {
        return Err(PendingWorkError::ExpectedOpenTaskMarker {
            note: item.note,
            line: item.line,
        });
    }
    let line_end = content[item.marker_index..]
        .find(['\r', '\n'])
        .map(|i| item.marker_index + i)
        .unwrap_or(content.len());
    let line_text = &content[item.marker_index..line_end];
    let mut checked_line = format!("- [x]{}", &line_text[5..]);
    // Add checkmark if not already present
    if !crate::regexes::DATE_STAMP_RE.is_match(&checked_line) {
        checked_line.push_str(&format!(" ✅ {date}"));
    }
    let updated = format!(
        "{}{}{}",
        &content[..item.marker_index],
        checked_line,
        &content[line_end..]
    );
    ObsidianStore::write_note(note_path, &updated)?;

    Ok(format!(
        "{} {} ({} :: {})\n",
        action.past_tense(),
        item.id,
        item.project,
        item.session
    ))
}

#[cfg(test)]
mod tests {
    use std::assert_matches;

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

    fn stage_file_item() -> (std::path::PathBuf, Config) {
        let stage = std::env::temp_dir().join(format!("pwf_check_{}", nanos()));
        let notes = stage.join("notes");
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

    fn nanos() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }

    #[test]
    fn missing_id_returns_typed_error_with_legacy_display() {
        let (_stage, cfg) = stage_file_item();

        let err = run_check(&cfg, &Args::default()).unwrap_err();

        assert_matches!(
            err,
            PendingWorkError::MissingId { action } if action == "check"
        );
        assert_eq!(err.to_string(), "--id is required for check.");
    }

    #[test]
    fn empty_report_returns_typed_error_with_legacy_display() {
        let (_stage, cfg) = stage_file_item();
        let args = Args {
            id: Some("GLP-0001".to_string()),
            report: Some(" \t\n".to_string()),
            ..Args::default()
        };

        let err = run_check(&cfg, &args).unwrap_err();

        assert_matches!(err, PendingWorkError::EmptyReport);
        assert_eq!(err.to_string(), "--report cannot be empty.");
    }

    #[test]
    fn check_with_review_appends_review_task_as_text() {
        // --review should append the added task as text, never JSON (PWF-0059).
        let (_stage, cfg) = stage_file_item();
        let args = Args {
            id: Some("GLP-0001".to_string()),
            review: true,
            date: Some("2026-01-01".to_string()),
            ..Args::default()
        };

        let out = run_check(&cfg, &args).unwrap();

        assert!(out.starts_with("Checked GLP-0001"), "got: {out}");
        assert!(out.contains("ADDED PWF TASK ["), "got: {out}");
    }
}
