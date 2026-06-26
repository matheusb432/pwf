// Action: update.

use std::{path::Path, sync::LazyLock};

use regex::Regex;

static TITLE_LINE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^title:.*$").unwrap());

use super::super::{
    commits,
    errors::PendingWorkError,
    index::{set_commits_text, set_prereq_text},
    model::Item,
    obsidian::store::ObsidianStore,
    prereq,
    query::{find_item_note_file, find_pending_item},
    text::{normalize_title, note_body},
};
use crate::{cli::Args, config::Config};

/// Replace the body (after frontmatter) with `body`, preserving the frontmatter
/// block byte-for-byte. Mirrors `work_item_content`'s `---\n\n<body>\n` shape.
fn replace_body(content: &str, body: &str) -> String {
    let mut fences = crate::regexes::FRONTMATTER_FENCE_RE.find_iter(content);
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
    let commits_value = commits::frontmatter_value(&args.commits);
    // Body edits (title/prompt/prereq) need the parsed open Item; commits only need
    // the note file, so it can be amended on closed (done/cancelled) items too.
    let edits_body = args.prompt.is_some()
        || args.title.is_some()
        || !args.prereq.is_empty()
        || args.clear_prereq;
    if !edits_body && commits_value.is_none() {
        return Err(PendingWorkError::NothingToUpdate);
    }

    match find_pending_item(cfg, id) {
        Ok(item) => update_open_item(cfg, args, &item, commits_value.as_deref()),
        // Closed items are skipped by the index parser; allow a commits-only amend
        // via the note file on disk (PWF-0062). Body edits still require an open item.
        Err(PendingWorkError::ItemNotFound { id }) => match find_item_note_file(cfg, &id) {
            Some(_) if edits_body => Err(PendingWorkError::ClosedItemCommitsOnly { id }),
            Some(path) => amend_commits_only(&path, commits_value.as_deref(), &id),
            None => Err(PendingWorkError::ItemNotFound { id }),
        },
        Err(other) => Err(other),
    }
}

/// Apply title/prompt/prereq and/or commits edits to an open file-model item.
fn update_open_item(
    cfg: &Config,
    args: &Args,
    item: &Item,
    commits_value: Option<&str>,
) -> Result<String, PendingWorkError> {
    let item_file = item
        .item_file
        .as_deref()
        .ok_or(PendingWorkError::UpdateRequiresFileModel)?;
    let item_path = Path::new(item_file);
    let mut content = ObsidianStore::read_item_file(item_path)?;

    let new_title = args
        .title
        .as_deref()
        .map(normalize_title)
        .unwrap_or_else(|| item.session.clone());
    if args.title.is_some() {
        content = TITLE_LINE_RE
            .replace(&content, format!("title: {new_title}").as_str())
            .into_owned();
    }
    if let Some(p) = args.prompt.as_deref() {
        content = replace_body(&content, &note_body(p));
    }
    if args.clear_prereq {
        content = set_prereq_text(&content, None);
    } else if !args.prereq.is_empty() {
        let merged = prereq::append_to_frontmatter(cfg, item.prereq.as_deref(), &args.prereq)?;
        content = set_prereq_text(&content, Some(&merged));
    }
    if let Some(range) = commits_value {
        content = set_commits_text(&content, Some(range));
    }

    ObsidianStore::write_item_file(item_path, &content)?;

    Ok(format!(
        "Updated {} ({} :: {})\n",
        item.id, item.project, new_title
    ))
}

/// Overwrite only the `commits:` provenance on a note file — no queue rotation,
/// no `completed:` re-stamp — so closed items' provenance can be corrected.
fn amend_commits_only(
    path: &Path,
    range: Option<&str>,
    id: &str,
) -> Result<String, PendingWorkError> {
    let content = ObsidianStore::read_item_file(path)?;
    let updated = set_commits_text(&content, range);
    ObsidianStore::write_item_file(path, &updated)?;
    Ok(format!("Updated {id} (commits: {})\n", range.unwrap_or("")))
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
            "nothing to update (pass --prompt, --title, --prereq, --clear-prereq, and/or --commits)."
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

        assert_matches!(err, PendingWorkError::UpdateRequiresFileModel);
        assert_eq!(
            err.to_string(),
            "update only supports file-model pending-work items."
        );
    }
}
