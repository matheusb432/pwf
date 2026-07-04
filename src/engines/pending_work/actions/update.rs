// Action: update.

use std::{path::Path, sync::LazyLock};

use regex::Regex;

static TITLE_LINE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^title:.*$").unwrap());

use super::super::{
    commits,
    errors::PendingWorkError,
    index::{
        append_lanes_text, append_report_block_text, set_commits_text, set_effort_text,
        set_prereq_text,
    },
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
    let append_report = args.append_report.as_deref();
    // Body edits (title/prompt/prereq/append) need the parsed open Item; `--commits`
    // and `--append-report` only touch the note file on disk, so they amend closed
    // (done/cancelled) items too.
    let edits_body = args.prompt.is_some()
        || args.title.is_some()
        || !args.prereq.is_empty()
        || args.clear_prereq
        || args.append.is_some()
        || args.effort.is_some();
    if !edits_body && commits_value.is_none() && append_report.is_none() {
        return Err(PendingWorkError::NothingToUpdate);
    }

    match find_pending_item(cfg, id) {
        Ok(item) => update_open_item(cfg, args, &item, commits_value.as_deref(), append_report),
        // Closed items are skipped by the index parser; allow a commits and/or
        // append-report amend via the note file on disk (PWF-0062, PWF-0065). Body
        // edits still require an open item.
        Err(PendingWorkError::ItemNotFound { id }) => match find_item_note_file(cfg, &id) {
            Some(_) if edits_body => Err(PendingWorkError::ClosedItemAmendOnly { id }),
            Some(path) => amend_closed_item(&path, commits_value.as_deref(), append_report, &id),
            None => Err(PendingWorkError::ItemNotFound { id }),
        },
        Err(other) => Err(other),
    }
}

/// Apply title/prompt/prereq, commits, and/or append-report edits to an open
/// file-model item.
fn update_open_item(
    cfg: &Config,
    args: &Args,
    item: &Item,
    commits_value: Option<&str>,
    append_report: Option<&str>,
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
    if let Some(p) = args.append.as_deref() {
        content = append_lanes_text(&content, p).ok_or(PendingWorkError::EmptyAppend)?;
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
    if let Some(tier) = args.effort {
        content = set_effort_text(&content, Some(tier));
    }
    if let Some(report) = append_report {
        content =
            append_report_block_text(&content, report).ok_or(PendingWorkError::EmptyReport)?;
    }

    ObsidianStore::write_item_file(item_path, &content)?;

    Ok(format!(
        "UPDATED PWF TASK [{}] {} :: {}\n",
        item.id, item.project, new_title
    ))
}

/// Apply the closed-item-safe amendments directly to the note file — overwrite the
/// `commits:` provenance and/or append a verbatim closeout report — with no queue
/// rotation, no `completed:` re-stamp, and no title/Goals regeneration, so a closed
/// item's provenance and narrative report can be corrected (PWF-0062, PWF-0065).
fn amend_closed_item(
    path: &Path,
    range: Option<&str>,
    append_report: Option<&str>,
    id: &str,
) -> Result<String, PendingWorkError> {
    let mut content = ObsidianStore::read_item_file(path)?;
    let mut changes: Vec<String> = Vec::new();
    if let Some(r) = range {
        content = set_commits_text(&content, Some(r));
        changes.push(format!("commits: {r}"));
    }
    if let Some(report) = append_report {
        content =
            append_report_block_text(&content, report).ok_or(PendingWorkError::EmptyReport)?;
        changes.push("report appended".to_string());
    }
    ObsidianStore::write_item_file(path, &content)?;
    Ok(format!("UPDATED PWF TASK [{id}] {}\n", changes.join(", ")))
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
            "nothing to update (pass --prompt, --title, --prereq, --clear-prereq, --commits, --append-report, --append, and/or --effort)."
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
