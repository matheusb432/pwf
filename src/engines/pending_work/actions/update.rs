// Action: update.

use super::super::errors::PendingWorkError;
use super::super::index::set_prereq_text;
use super::super::obsidian::store::ObsidianStore;
use super::super::prereq;
use super::super::query::find_pending_item;
use super::super::text::{normalize_title, note_body};
use crate::cli::Args;
use crate::config::Config;
use regex::Regex;
use std::path::Path;

/// Replace the body (after frontmatter) with `body`, preserving the frontmatter
/// block byte-for-byte. Mirrors `work_item_content`'s `---\n\n<body>\n` shape.
fn replace_body(content: &str, body: &str) -> String {
    let fence_re = Regex::new(r"(?m)^---[ \t]*$").unwrap();
    let mut fences = fence_re.find_iter(content);
    // Opening + closing `---` lines delimit the frontmatter; body follows the closer.
    match (fences.next(), fences.next()) {
        (Some(_), Some(close)) => {
            format!("{}\n\n{}\n", &content[..close.end()], body.trim_end())
        }
        // No frontmatter pair: replace the whole content with just the body.
        _ => format!("{}\n", body.trim_end()),
    }
}

pub(in crate::engines::pending_work) fn run_update(
    cfg: &Config,
    args: &Args,
) -> Result<String, PendingWorkError> {
    let id = args
        .id
        .as_deref()
        .ok_or(PendingWorkError::MissingId { action: "update" })?;
    let prompt = args.prompt.as_deref();
    let title = args.title.as_deref();
    if prompt.is_none() && title.is_none() && args.prereq.is_empty() && !args.clear_prereq {
        return Err(PendingWorkError::NothingToUpdate);
    }

    let item = find_pending_item(cfg, id)?;
    let item_file = item
        .item_file
        .as_deref()
        .ok_or(PendingWorkError::UpdateRequiresFileModel)?;
    let item_path = Path::new(item_file);
    let mut content = ObsidianStore::read_item_file(item_path)?;

    let new_title = title
        .map(normalize_title)
        .unwrap_or_else(|| item.session.clone());
    if title.is_some() {
        let title_re = Regex::new(r"(?m)^title:.*$").unwrap();
        content = title_re
            .replace(&content, format!("title: {new_title}").as_str())
            .into_owned();
    }
    if let Some(p) = prompt {
        content = replace_body(&content, &note_body(p));
    }
    if args.clear_prereq {
        content = set_prereq_text(&content, None);
    } else if !args.prereq.is_empty() {
        let merged = prereq::append_to_frontmatter(cfg, item.prereq.as_deref(), &args.prereq)?;
        content = set_prereq_text(&content, Some(&merged));
    }

    ObsidianStore::write_item_file(item_path, &content)?;

    if args.json {
        let obj = serde_json::json!({
            "id": item.id,
            "project": item.project,
            "session": new_title,
            "note": item_file,
            "status": "updated"
        });
        return Ok(serde_json::to_string_pretty(&obj).unwrap());
    }
    Ok(format!(
        "Updated {} ({} :: {})\n",
        item.id, item.project, new_title
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

    fn stage_legacy_item() -> (std::path::PathBuf, Config) {
        let stage = std::env::temp_dir().join(format!("pwf_update_{}", nanos()));
        let notes = stage.join("notes");
        let project = notes.join("glep-shimeji");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("glep-shimeji.md"), "- [ ] `legacy` :: do it\n").unwrap();
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
        let (_stage, cfg) = stage_legacy_item();
        let args = Args {
            prompt: Some("x".to_string()),
            ..Args::default()
        };

        let err = run_update(&cfg, &args).unwrap_err();

        assert!(matches!(
            err,
            PendingWorkError::MissingId { action } if action == "update"
        ));
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

        assert!(matches!(err, PendingWorkError::NothingToUpdate));
        assert_eq!(
            err.to_string(),
            "nothing to update (pass --prompt, --title, --prereq, and/or --clear-prereq)."
        );
    }

    #[test]
    fn missing_item_file_returns_typed_error_with_legacy_display() {
        let (_stage, cfg) = stage_legacy_item();
        let args = Args {
            id: Some("glep-shimeji:1".to_string()),
            prompt: Some("x".to_string()),
            ..Args::default()
        };

        let err = run_update(&cfg, &args).unwrap_err();

        assert!(matches!(err, PendingWorkError::UpdateRequiresFileModel));
        assert_eq!(
            err.to_string(),
            "update only supports file-model pending-work items."
        );
    }
}
