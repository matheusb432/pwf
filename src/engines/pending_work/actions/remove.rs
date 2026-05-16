// Action: remove.

use super::super::errors::PendingWorkError;
use super::super::index::remove_index_link;
use super::super::naming::project_index_path;
use super::super::obsidian::store::ObsidianStore;
use super::super::query::find_pending_item;
use crate::cli::Args;
use crate::config::Config;
use std::path::Path;

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

pub(in crate::engines::pending_work) fn run_remove(
    cfg: &Config,
    args: &Args,
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

    fn stage_file_item(index: &str) -> (std::path::PathBuf, Config) {
        let stage = std::env::temp_dir().join(format!("pwf_remove_{}", nanos()));
        let notes = stage.join("notes");
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

    fn nanos() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }

    #[test]
    fn missing_id_returns_typed_error_with_legacy_display() {
        let (_stage, cfg) = stage_file_item("- [ ] [[GLP-0001]]\n");

        let err = run_remove(&cfg, &Args::default()).unwrap_err();

        assert!(matches!(
            err,
            PendingWorkError::MissingId { action } if action == "remove"
        ));
        assert_eq!(err.to_string(), "--id is required for remove.");
    }

    #[test]
    fn missing_item_file_returns_typed_error_with_legacy_display() {
        let stage = std::env::temp_dir().join(format!("pwf_remove_legacy_{}", nanos()));
        let notes = stage.join("notes");
        let project = notes.join("glep-shimeji");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("glep-shimeji.md"), "- [ ] `legacy` :: do it\n").unwrap();
        let cfg = cfg(&notes);
        let args = Args {
            id: Some("glep-shimeji:1".to_string()),
            ..Args::default()
        };

        let err = run_remove(&cfg, &args).unwrap_err();

        assert!(matches!(err, PendingWorkError::RemoveRequiresFileModel));
        assert_eq!(
            err.to_string(),
            "remove only supports file-model pending-work items."
        );
    }

    #[test]
    fn missing_note_returns_typed_error_with_legacy_display() {
        let (stage, cfg) = stage_file_item("- [ ] [[GLP-0001]]\n");
        let missing = stage.join("notes/glep-shimeji/GLP-0001.md");
        std::fs::remove_file(&missing).unwrap();
        let args = Args {
            id: Some("GLP-0001".to_string()),
            ..Args::default()
        };

        let err = run_remove(&cfg, &args).unwrap_err();

        assert!(matches!(
            err,
            PendingWorkError::WorkItemNoteMissing { ref path } if path == &missing
        ));
        assert_eq!(
            err.to_string(),
            format!("Work-item note missing: {}", missing.display())
        );
    }

    #[test]
    fn missing_index_link_returns_typed_error_with_legacy_display() {
        let err = remove_link_from_index_content("- [ ] [[GLP-9999]]\n", "GLP-0001").unwrap_err();

        assert!(matches!(
            err,
            PendingWorkError::IndexLinkNotFound { ref id } if id == "GLP-0001"
        ));
        assert_eq!(err.to_string(), "Index link not found for GLP-0001.");
    }
}
