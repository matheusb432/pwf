// Leaf action implementations: check (mark done), remove, and list.

use super::add::{NewItemSpec, add_pending_work_item};
use super::commits;
use super::index::{append_report_text, remove_index_link, set_commits_text, set_status_text};
use super::model::Item;
use super::naming::{project_dir, project_index_path, stamp_date};
use super::prereq;
use super::query::{find_pending_item, get_pending_work};
use super::section::Section;
use crate::cli::Args;
use crate::config::Config;
use regex::Regex;
use std::path::Path;

// ── Action: check ─────────────────────────────────────────────────────────────

pub(super) fn run_check(cfg: &Config, args: &Args) -> Result<String, String> {
    let id = args.id.as_deref().ok_or("--id is required for check.")?;
    let item = find_pending_item(cfg, id)?;
    let date = stamp_date(&args.date);

    if let Some(ref file) = item.item_file {
        // File-model path
        let item_path = Path::new(file);
        let content = std::fs::read_to_string(item_path)
            .map_err(|e| format!("Cannot read item file: {e}"))?;
        let content = if let Some(report) = args.report.as_deref() {
            append_report_text(&content, report).ok_or("--report cannot be empty.")?
        } else {
            content
        };
        // Record commit-range provenance only when supplied — the default path stays
        // byte-identical so the frozen conformance goldens don't move (PWF-0017).
        let commits_value = commits::frontmatter_value(&args.commits);
        let content = match commits_value.as_deref() {
            Some(v) => set_commits_text(&content, Some(v)),
            None => content,
        };
        let updated = set_status_text(&content, "done", &date);
        crate::fs_atomic::write_text_atomic(item_path, &updated)
            .map_err(|e| format!("Cannot write item file: {e}"))?;

        // Keep the item in the index as a capped, rotating done-queue (PWF-0026).
        let notes_dir = cfg.notes_dir_for(&item.project);
        let index_path = project_index_path(notes_dir, &item.project);
        if index_path.exists() {
            let idx_content = std::fs::read_to_string(&index_path)
                .map_err(|e| format!("Cannot read index: {e}"))?;
            let queue = super::done_queue::mark_done(&idx_content, id, &date);
            crate::fs_atomic::write_text_atomic(&index_path, &queue.content)
                .map_err(|e| format!("Cannot write index: {e}"))?;
            if queue.futuro_renamed {
                eprintln!(
                    "info: normalized `## Futuro` header to `## Future` in {}",
                    item.project
                );
            }
            let dir = project_dir(notes_dir, &item.project);
            for ev in &queue.evicted {
                archive_item_file(&dir, ev)?;
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
                    // ? mirror the caller's format so --json nests an object, not a text block.
                    json: args.json,
                    prereq: None,
                },
            )?)
        } else {
            None
        };

        if args.json {
            // The spawned add returns JSON here (json: args.json), so nest it as an
            // object rather than a string to match the rest of the surface (PWF-0017).
            let review_obj = review.as_deref().map(|r| {
                serde_json::from_str(r).unwrap_or_else(|_| serde_json::Value::String(r.to_string()))
            });
            let obj = serde_json::json!({
                "id": item.id,
                "project": item.project,
                "session": item.session,
                "note": file,
                "status": "checked",
                "reviewTask": review_obj,
            });
            return Ok(serde_json::to_string_pretty(&obj).unwrap());
        }
        let mut out = format!(
            "Checked {} ({} :: {})\n",
            item.id, item.project, item.session
        );
        if let Some(review) = review {
            out.push_str(&review);
        }
        return Ok(out);
    }

    // Legacy checkbox path
    let note_path = Path::new(&item.note);
    let content =
        std::fs::read_to_string(note_path).map_err(|e| format!("Cannot read note: {e}"))?;
    // Bounds-checked slice: if the note shrank since it was listed, return the
    // "may have changed" error rather than panicking on the out-of-range slice.
    let marker = content
        .get(item.marker_index..item.marker_index + 5)
        .unwrap_or("");
    if marker != "- [ ]" {
        return Err(format!(
            "Expected open task marker at {}:{}. The note may have changed.",
            item.note, item.line
        ));
    }
    let line_end = content[item.marker_index..]
        .find(['\r', '\n'])
        .map(|i| item.marker_index + i)
        .unwrap_or(content.len());
    let line_text = &content[item.marker_index..line_end];
    let mut checked_line = format!("- [x]{}", &line_text[5..]);
    // Add checkmark if not already present
    let checkmark_re = Regex::new(r"✅\s*\d{4}-\d{2}-\d{2}").unwrap();
    if !checkmark_re.is_match(&checked_line) {
        checked_line.push_str(&format!(" ✅ {date}"));
    }
    let updated = format!(
        "{}{}{}",
        &content[..item.marker_index],
        checked_line,
        &content[line_end..]
    );
    crate::fs_atomic::write_text_atomic(note_path, &updated)
        .map_err(|e| format!("Cannot write note: {e}"))?;

    if args.json {
        let obj = serde_json::json!({
            "id": item.id,
            "project": item.project,
            "session": item.session,
            "note": item.note,
            "line": item.line,
            "status": "checked"
        });
        return Ok(serde_json::to_string_pretty(&obj).unwrap());
    }
    Ok(format!(
        "Checked {} ({} :: {})\n",
        item.id, item.project, item.session
    ))
}

/// Move an evicted item's note out of the project dir into `_archive/`,
/// preserving its done report. A missing source is a no-op.
fn archive_item_file(project_dir: &Path, id: &str) -> Result<(), String> {
    let src = project_dir.join(format!("{id}.md"));
    if !src.exists() {
        return Ok(());
    }
    let archive_dir = project_dir.join("_archive");
    std::fs::create_dir_all(&archive_dir).map_err(|e| format!("Cannot create archive dir: {e}"))?;
    std::fs::rename(&src, archive_dir.join(format!("{id}.md")))
        .map_err(|e| format!("Cannot archive {id}: {e}"))?;
    Ok(())
}

// ── Action: update ────────────────────────────────────────────────────────────

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

pub(super) fn run_update(cfg: &Config, args: &Args) -> Result<String, String> {
    let id = args.id.as_deref().ok_or("--id is required for update.")?;
    let prompt = args.prompt.as_deref();
    let title = args.title.as_deref();
    if prompt.is_none() && title.is_none() && args.prereq.is_empty() && !args.clear_prereq {
        return Err(
            "nothing to update (pass --prompt, --title, --prereq, and/or --clear-prereq)."
                .to_string(),
        );
    }

    let item = find_pending_item(cfg, id)?;
    let item_file = item
        .item_file
        .as_deref()
        .ok_or("update only supports file-model pending-work items.")?;
    let item_path = Path::new(item_file);
    let mut content =
        std::fs::read_to_string(item_path).map_err(|e| format!("Cannot read item file: {e}"))?;

    let new_title = title
        .map(super::text::normalize_title)
        .unwrap_or_else(|| item.session.clone());
    if title.is_some() {
        let title_re = Regex::new(r"(?m)^title:.*$").unwrap();
        content = title_re
            .replace(&content, format!("title: {new_title}").as_str())
            .into_owned();
    }
    if let Some(p) = prompt {
        content = replace_body(&content, &super::text::note_body(p));
    }
    if args.clear_prereq {
        content = super::index::set_prereq_text(&content, None);
    } else if !args.prereq.is_empty() {
        let merged = prereq::append_to_frontmatter(cfg, item.prereq.as_deref(), &args.prereq)?;
        content = super::index::set_prereq_text(&content, Some(&merged));
    }

    crate::fs_atomic::write_text_atomic(item_path, &content)
        .map_err(|e| format!("Cannot write item file: {e}"))?;

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

// ── Action: remove ────────────────────────────────────────────────────────────

pub(super) fn run_remove(cfg: &Config, args: &Args) -> Result<String, String> {
    let id = args.id.as_deref().ok_or("--id is required for remove.")?;

    let item = find_pending_item(cfg, id)?;
    let item_file = item
        .item_file
        .as_deref()
        .ok_or("remove only supports file-model pending-work items.")?;
    let item_path = Path::new(item_file);
    if !item_path.exists() {
        return Err(format!("Work-item note missing: {}", item_path.display()));
    }

    let index_path = project_index_path(cfg.notes_dir_for(&item.project), &item.project);
    let index_content =
        std::fs::read_to_string(&index_path).map_err(|e| format!("Cannot read index: {e}"))?;
    let removed = remove_index_link(&index_content, &item.id);
    if removed == index_content {
        return Err(format!("Index link not found for {}.", item.id));
    }
    crate::fs_atomic::write_text_atomic(&index_path, &removed)
        .map_err(|e| format!("Cannot write index: {e}"))?;
    std::fs::remove_file(item_path).map_err(|e| format!("Cannot remove item file: {e}"))?;

    if args.json {
        let obj = serde_json::json!({
            "id": item.id,
            "project": item.project,
            "title": item.session,
            "note": item.note,
            "itemFile": item_file,
            "status": "removed"
        });
        return Ok(serde_json::to_string_pretty(&obj).unwrap());
    }
    Ok(format!(
        "REMOVED PWF TASK [{}] {} :: {}\n  deleted: {}\n  unlinked: {}\n",
        item.id,
        item.project,
        item.session,
        item_path.display(),
        index_path.display()
    ))
}

// ── List selection: pure sort + cap + footer (no I/O) ──────────────────────────

/// Default item cap for `pw list` when `-n` is absent — keeps agents from being
/// flooded with tokens (PWF-0020). `-n 0` overrides to unlimited.
const DEFAULT_LIST_CAP: usize = 10;

/// Numeric ID suffix (digits after the last `-`), or 0 when unparseable. Zero-padded
/// per-prefix counters mean higher = newer inside a project group.
fn id_suffix(id: &str) -> u64 {
    id.rsplit_once('-')
        .and_then(|(_, n)| n.parse().ok())
        .unwrap_or(0)
}

/// Reorder items by project, then newest-first inside each project. The full id
/// string tiebreaks so the order is deterministic across runs.
fn sort_by_project_then_newest(items: &mut [Item]) {
    items.sort_by(|a, b| {
        a.project
            .cmp(&b.project)
            .then_with(|| id_suffix(&b.id).cmp(&id_suffix(&a.id)))
            .then_with(|| b.id.cmp(&a.id))
    });
}

/// Keep at most `cap` items (`cap == 0` ⇒ unlimited). Returns `(kept, hidden)`;
/// `kept.len() + hidden` always equals the input length.
fn apply_cap(items: Vec<Item>, cap: usize) -> (Vec<Item>, usize) {
    if cap == 0 || items.len() <= cap {
        return (items, 0);
    }
    let hidden = items.len() - cap;
    let mut kept = items;
    kept.truncate(cap);
    (kept, hidden)
}

/// "More" footer (no trailing newline); empty when nothing is hidden. Mentions the
/// hidden count and the `-n 0` escape hatch. ASCII-only for Linux+Windows consoles.
fn more_footer(hidden: usize) -> String {
    if hidden == 0 {
        return String::new();
    }
    format!("... and {hidden} more; run 'pwf pw -n 0' to show all")
}

/// List action implementation. `## Future` and `## Human` items are hidden unless
/// `show_future` / `show_human` re-include them.
pub(super) fn run_list_action(
    cfg: &Config,
    only_project: Option<&str>,
    json: bool,
    long: bool,
    show_future: bool,
    show_human: bool,
    number: Option<usize>,
) -> Result<String, String> {
    let mut items: Vec<_> = get_pending_work(cfg, only_project)?
        .into_iter()
        .filter(|i| match i.section.as_deref() {
            Some("Future") => show_future,
            Some("Human") => show_human,
            _ => true,
        })
        .collect();
    // Order + cap before the JSON branch so `--json` follows the same selection.
    sort_by_project_then_newest(&mut items);
    let (items, hidden) = apply_cap(items, number.unwrap_or(DEFAULT_LIST_CAP));
    if json {
        return Ok(serde_json::to_string_pretty(&items).unwrap());
    }
    if items.is_empty() {
        let target = only_project
            .map(|p| format!("{p} in {}", cfg.notes_dir))
            .unwrap_or_else(|| cfg.notes_dir.clone());
        return Ok(format!("No open pending-work prompts found in {target}.\n"));
    }
    let mut out = String::new();
    let last_item = items.last().unwrap();
    for item in &items {
        let formatted_str = if item != last_item {
            // list item format. e.g. [PWF-0001] pwf :: some title
            format!("[{}] {} :: {}\n", item.id, item.project, item.session)
        } else {
            // last list item format that must not include line breaks. signed - human :)
            format!("[{}] {} :: {}", item.id, item.project, item.session)
        };

        out.push_str(&formatted_str);
        if !long {
            continue;
        }
        // --long metadata.
        let status = if item.launchable {
            "READY"
        } else {
            "NEEDS ATTENTION"
        };
        out.push_str(&format!("  status: {status}\n"));
        if item.needs_prompt {
            out.push_str("  status: NEEDS PROMPT\n");
        }
        match &item.repo {
            Some(r) if !r.is_empty() => out.push_str(&format!("  repo: {r}\n")),
            _ => out.push_str("  repo: (not configured)\n"),
        }
        out.push_str(&format!("  note: {}:{}\n", item.note, item.line));
        let prompt = item.prompt.replace("\r\n", " / ").replace('\n', " / ");
        out.push_str(&format!("  prompt: {prompt}\n"));
        if let Some(pq) = &item.prereq {
            let statuses = prereq::resolve(cfg, pq);
            if !statuses.is_empty() {
                out.push_str(&format!("  prereq: {}\n", prereq::list_summary(&statuses)));
            }
        }
        for issue in &item.issues {
            out.push_str(&format!("  issue: {issue}\n"));
        }
        if !item.launchable {
            out.push_str(&format!(
                "  fix: edit {} or config/pending-work.json\n",
                item.note
            ));
        }
    }
    if hidden > 0 {
        // ? Short mode leaves the last item without a trailing newline (preserved when
        // ? nothing is hidden); add one only here so the footer sits on its own line.
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&more_footer(hidden));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal `Item` carrying only the `id` the pure helpers read.
    fn item(id: &str) -> Item {
        Item {
            id: id.to_string(),
            project: "glep-shimeji".to_string(),
            session: "t".to_string(),
            prompt: String::new(),
            repo: None,
            note: String::new(),
            item_file: None,
            line: 0,
            format: String::new(),
            marker_index: 0,
            marker_length: 0,
            launchable: true,
            needs_prompt: false,
            issues: vec![],
            section: None,
            prereq: None,
        }
    }

    fn ids(items: &[Item]) -> Vec<&str> {
        items.iter().map(|i| i.id.as_str()).collect()
    }

    #[test]
    fn sort_by_project_then_newest_orders_one_project_by_descending_id() {
        let mut items = vec![item("GLP-0001"), item("GLP-0003"), item("GLP-0002")];
        sort_by_project_then_newest(&mut items);
        assert_eq!(ids(&items), ["GLP-0003", "GLP-0002", "GLP-0001"]);
    }

    #[test]
    fn sort_by_project_then_newest_groups_by_project_before_descending_id() {
        let mut cfg_item = item("CFG-0001");
        cfg_item.project = "config-handler".to_string();
        let mut pwf_item = item("PWF-9999");
        pwf_item.project = "pwf".to_string();
        let mut cfg_newer_item = item("CFG-0002");
        cfg_newer_item.project = "config-handler".to_string();

        let mut items = vec![pwf_item, cfg_item, cfg_newer_item];
        sort_by_project_then_newest(&mut items);

        assert_eq!(ids(&items), ["CFG-0002", "CFG-0001", "PWF-9999"]);
    }

    #[test]
    fn apply_cap_default_keeps_first_n_after_sort() {
        let items: Vec<Item> = (1..=12).map(|n| item(&format!("GLP-{n:04}"))).collect();
        let (kept, hidden) = apply_cap(items, DEFAULT_LIST_CAP);
        assert_eq!(kept.len(), 10);
        assert_eq!(hidden, 2);
    }

    #[test]
    fn apply_cap_zero_is_unlimited() {
        let items: Vec<Item> = (1..=12).map(|n| item(&format!("GLP-{n:04}"))).collect();
        let (kept, hidden) = apply_cap(items, 0);
        assert_eq!(kept.len(), 12);
        assert_eq!(hidden, 0);
    }

    #[test]
    fn apply_cap_larger_than_len_keeps_all() {
        let items = vec![item("GLP-0001"), item("GLP-0002"), item("GLP-0003")];
        let (kept, hidden) = apply_cap(items, 10);
        assert_eq!(kept.len(), 3);
        assert_eq!(hidden, 0);
    }

    #[test]
    fn more_footer_empty_when_nothing_hidden() {
        assert_eq!(more_footer(0), "");
    }

    #[test]
    fn more_footer_mentions_count_and_escape_hatch() {
        let footer = more_footer(2);
        assert!(footer.contains("2 more"), "got: {footer}");
        assert!(footer.contains("-n 0"), "got: {footer}");
    }
}
