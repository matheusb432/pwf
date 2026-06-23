// Write-side store: create a pending-work item file + link it into the index.

use super::super::errors::PendingWorkError;
use super::super::index::{
    add_link_to_index, add_section_block, section_exists, work_item_content,
};
use super::super::naming::{next_work_item_id, project_dir, project_index_path, project_key};
use super::super::obsidian::store::ObsidianStore;
use super::super::section::Section;
use super::super::text::{inferred_title, normalize_title, note_body};
use crate::config::Config;
use std::path::Path;

/// The inputs for creating one pending-work item. Grouped so the writer takes a
/// single spec instead of a long positional argument list.
#[derive(Debug, Clone, Copy)]
pub(in crate::engines::pending_work) struct NewItemSpec<'a> {
    pub project_name: &'a str,
    pub task_prompt: &'a str,
    pub task_title: Option<&'a str>,
    pub created: &'a str,
    pub section: Option<Section>,
    pub prereq: Option<&'a str>,
}

/// Create a pending-work item file and link it into the project index.
///
/// # Errors
///
/// Returns an error string if the project is unmanaged, a notes/index directory
/// cannot be created, or a note/index file cannot be written.
pub(in crate::engines::pending_work) fn add_pending_work_item(
    cfg: &Config,
    spec: &NewItemSpec,
) -> Result<String, PendingWorkError> {
    let NewItemSpec {
        project_name,
        task_prompt,
        task_title,
        created,
        section,
        prereq,
    } = *spec;
    let repo = cfg
        .projects
        .get(project_name)
        .map(|s| s.as_str())
        .unwrap_or("");
    if repo.trim().is_empty() {
        return Err(PendingWorkError::ProjectNotMappedToRepo {
            project: project_name.to_string(),
        });
    }
    let session = match task_title {
        Some(t) if !t.trim().is_empty() => normalize_title(t),
        _ => inferred_title(task_prompt),
    };

    let key =
        project_key(cfg, project_name).ok_or_else(|| PendingWorkError::ProjectMissingPrefix {
            project: project_name.to_string(),
        })?;
    let dir = project_dir(cfg.notes_dir_for(project_name), project_name);
    if !dir.exists() {
        ObsidianStore::create_project_dir(&dir)?;
    }
    let id = next_work_item_id(&dir, key);
    let item_path = dir.join(format!("{id}.md"));
    let body = note_body(task_prompt);
    let content = work_item_content(
        &session,
        project_name,
        &body,
        "active",
        created,
        None,
        prereq,
    );
    ObsidianStore::write_add_item_file(&item_path, &content)?;

    // Read/create the index, then write it.
    let index = project_index_path(cfg.notes_dir_for(project_name), project_name);
    let index_dir = index.parent().unwrap_or(Path::new("."));
    if !index_dir.exists() {
        ObsidianStore::create_index_dir(index_dir)?;
    }
    let existing = if index.exists() {
        ObsidianStore::read_text_or_default(&index)
    } else {
        String::new()
    };
    let link = format!("- [ ] [[{id}]]");
    let updated = if let Some(sec) = section {
        // Log when this add creates a section header that did not exist (PWF-0026).
        if !section_exists(&existing, sec) {
            eprintln!(
                "info: created `## {}` section in {project_name}",
                sec.as_str()
            );
        }
        add_section_block(&existing, &format!("{link}\n"), sec)
    } else {
        add_link_to_index(&existing, &link)
    };
    ObsidianStore::write_add_index_file(&index, &updated)?;

    let mut out = format!("ADDED PWF TASK [{id}] {project_name} :: {session}\n");
    out.push_str(&format!("  file: {}\n", item_path.display()));
    out.push_str(&format!("  dispatch with: pwf session {id}\n"));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::pending_work::errors::PendingWorkError;
    use crate::engines::pending_work::obsidian::store::StoreError;

    fn nanos() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    }

    fn temp_notes_dir(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("pwf_add_{name}_{}", nanos()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn cfg_for_notes(notes_dir: &std::path::Path) -> crate::config::Config {
        let notes_json = serde_json::to_string(&notes_dir.to_string_lossy()).unwrap();
        crate::config::from_json(
            &format!(
                r#"{{ "notesDir": {notes_json}, "projects": {{ "pwf": "/repo" }}, "prefixes": {{ "pwf": "PWF" }} }}"#
            ),
            None,
        )
        .unwrap()
    }

    fn spec() -> NewItemSpec<'static> {
        NewItemSpec {
            project_name: "pwf",
            task_prompt: "do it",
            task_title: None,
            created: "2026-01-01",
            section: None,
            prereq: None,
        }
    }

    #[test]
    fn unmapped_project_returns_typed_error_with_legacy_display() {
        let cfg = crate::config::from_json(
            r#"{ "notesDir": "/tmp/notes", "projects": { "pwf": "" }, "prefixes": { "pwf": "PWF" } }"#,
            None,
        )
        .unwrap();
        let spec = spec();

        let err = add_pending_work_item(&cfg, &spec).unwrap_err();

        assert!(matches!(
            err,
            PendingWorkError::ProjectNotMappedToRepo { ref project } if project == "pwf"
        ));
        assert_eq!(
            err.to_string(),
            "Project 'pwf' is not mapped to a repo in config/pending-work.json."
        );
    }

    #[test]
    fn add_item_write_error_preserves_legacy_display() {
        let notes_dir = temp_notes_dir("item_write");
        std::fs::write(notes_dir.join("pwf"), "not a dir").unwrap();
        let cfg = cfg_for_notes(&notes_dir);
        let spec = spec();

        let err = add_pending_work_item(&cfg, &spec).unwrap_err();

        assert!(matches!(
            err,
            PendingWorkError::Store(StoreError::AddWriteItemFile { .. })
        ));
        assert!(err.to_string().starts_with("Failed to write item file: "));
    }

    #[test]
    fn add_index_write_error_preserves_legacy_display() {
        let notes_dir = temp_notes_dir("index_write");
        let project_dir = notes_dir.join("pwf");
        std::fs::create_dir_all(project_dir.join("pwf.md")).unwrap();
        let cfg = cfg_for_notes(&notes_dir);
        let spec = spec();

        let err = add_pending_work_item(&cfg, &spec).unwrap_err();

        assert!(matches!(
            err,
            PendingWorkError::Store(StoreError::AddWriteIndexFile { .. })
        ));
        assert!(err.to_string().starts_with("Failed to write index file: "));
    }
}
