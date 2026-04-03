// Write-side store: create a pending-work item file + link it into the index.

use super::errors;
use super::index::{add_link_to_index, add_section_block, work_item_content};
use super::naming::{next_work_item_id, path_str, project_dir, project_index_path, project_key};
use super::section::Section;
use super::text::{inferred_title, normalize_title, note_body};
use crate::config::Config;
use std::path::Path;

/// The inputs for creating one pending-work item. Grouped so the writer takes a
/// single spec instead of a long positional argument list.
#[derive(Debug, Clone, Copy)]
pub(super) struct NewItemSpec<'a> {
    pub project_name: &'a str,
    pub task_prompt: &'a str,
    pub task_title: Option<&'a str>,
    pub created: &'a str,
    pub section: Option<Section>,
    pub prereq: Option<&'a str>,
    pub json: bool,
}

/// Create a pending-work item file and link it into the project index.
///
/// # Errors
///
/// Returns an error string if the project is unmanaged, a notes/index directory
/// cannot be created, or a note/index file cannot be written.
pub(super) fn add_pending_work_item(cfg: &Config, spec: &NewItemSpec) -> Result<String, String> {
    let NewItemSpec {
        project_name,
        task_prompt,
        task_title,
        created,
        section,
        prereq,
        json,
    } = *spec;
    let repo = cfg
        .projects
        .get(project_name)
        .map(|s| s.as_str())
        .unwrap_or("");
    if repo.trim().is_empty() {
        return Err(errors::not_mapped_to_repo(project_name));
    }
    let session = match task_title {
        Some(t) if !t.trim().is_empty() => normalize_title(t),
        _ => inferred_title(task_prompt),
    };

    let key = project_key(cfg, project_name)?;
    let dir = project_dir(cfg.notes_dir_for(project_name), project_name);
    if !dir.exists() {
        std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create project dir: {e}"))?;
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
    crate::fs_atomic::write_text_atomic(&item_path, &content)
        .map_err(|e| format!("Failed to write item file: {e}"))?;

    // Read/create the index, then write it.
    let index = project_index_path(cfg.notes_dir_for(project_name), project_name);
    let index_dir = index.parent().unwrap_or(Path::new("."));
    if !index_dir.exists() {
        std::fs::create_dir_all(index_dir).map_err(|e| format!("Cannot create index dir: {e}"))?;
    }
    let existing = if index.exists() {
        std::fs::read_to_string(&index).unwrap_or_default()
    } else {
        String::new()
    };
    let link = format!("- [ ] [[{id}]]");
    let updated = if let Some(sec) = section {
        // Log when this add creates a section header that did not exist (PWF-0026).
        if !super::index::section_exists(&existing, sec) {
            eprintln!(
                "info: created `## {}` section in {project_name}",
                sec.as_str()
            );
        }
        add_section_block(&existing, &format!("{link}\n"), sec)
    } else {
        add_link_to_index(&existing, &link)
    };
    crate::fs_atomic::write_text_atomic(&index, &updated)
        .map_err(|e| format!("Failed to write index file: {e}"))?;

    if json {
        let obj = serde_json::json!({
            "id": id,
            "project": project_name,
            "title": session,
            "session": session,
            "prompt": task_prompt,
            "repo": repo,
            "note": path_str(&index),
            "itemFile": path_str(&item_path),
            "status": "added"
        });
        return Ok(serde_json::to_string_pretty(&obj).unwrap());
    }

    // Text form
    let mut out = format!("ADDED PWF TASK [{id}] {project_name} :: {session}\n");
    out.push_str(&format!("  file: {}\n", item_path.display()));
    out.push_str(&format!("  launch with: pwf pw launch --id {id}\n"));
    Ok(out)
}
