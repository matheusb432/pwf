// Action: update.

use cqrsy::send_now;
use pwf_application::{UpdatePendingWorkItem, UpdatePendingWorkItemHandler};
use pwf_infra::obsidian::ObsidianPendingWorkStore;

use super::{
    super::{commits, errors::PendingWorkError},
    outcome::UpdatedItem,
};
use crate::{cli::Args, config::Config};

pub(in crate::engines::pending_work) fn run_update(
    cfg: &Config,
    args: &Args,
) -> Result<UpdatedItem, PendingWorkError> {
    let id = args
        .id
        .as_deref()
        .ok_or(PendingWorkError::MissingId { action: "update" })?;
    let commits = commits::frontmatter_value(&args.commits);
    let tags = super::super::tags::from_flags(&args.tag)?;
    let edits_body = args.prompt.is_some()
        || args.title.is_some()
        || !args.prereq.is_empty()
        || args.clear_prereq
        || args.append.is_some()
        || args.effort.is_some()
        || tags.is_some()
        || args.tags_clear;
    if !edits_body && commits.is_none() && args.append_report.is_none() {
        return Err(PendingWorkError::NothingToUpdate);
    }

    let handler = UpdatePendingWorkItemHandler::new(ObsidianPendingWorkStore::new(cfg.clone()));
    send_now(
        &(),
        &handler,
        UpdatePendingWorkItem {
            id: id.to_string(),
            prompt: args.prompt.clone(),
            title: args.title.clone(),
            append: args.append.clone(),
            prereq: args.prereq.clone(),
            clear_prereq: args.clear_prereq,
            commits,
            append_report: args.append_report.clone(),
            effort: args.effort,
            tags,
            tags_clear: args.tags_clear,
        },
    )
    .map_err(|error| PendingWorkError::ApplicationWrite(error.to_string()))
}

#[cfg(test)]
mod tests {
    use std::{assert_matches, path::Path};

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

    fn stage_legacy_item() -> (tempfile::TempDir, Config) {
        let stage = tempfile::tempdir().unwrap();
        let notes = stage.path().join("notes");
        let project = notes.join("glep-shimeji");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("glep-shimeji.md"), "- [ ] `legacy` :: do it\n").unwrap();
        let cfg = cfg(&notes);
        (stage, cfg)
    }

    #[test]
    fn missing_id_returns_typed_error_with_legacy_display() {
        let (_stage, cfg) = stage_legacy_item();
        let args = Args {
            prompt: Some("x".to_string()),
            ..Args::default()
        };

        let err = run_update(&cfg, &args).unwrap_err();

        assert_matches!(
            err,
            PendingWorkError::MissingId { action } if action == "update"
        );
        assert_eq!(err.to_string(), "--id is required for update.");
    }

    #[test]
    fn nothing_to_update_returns_typed_error_with_legacy_display() {
        let (_stage, cfg) = stage_legacy_item();
        let args = Args {
            id: Some("glep-shimeji:1".to_string()),
            ..Args::default()
        };

        let err = run_update(&cfg, &args).unwrap_err();

        assert_matches!(err, PendingWorkError::NothingToUpdate);
        assert_eq!(
            err.to_string(),
            "nothing to update (pass --prompt, --title, --prereq, --clear-prereq, --tag, --tags-clear, --commits, --append-report, --append, and/or --effort)."
        );
    }
}
