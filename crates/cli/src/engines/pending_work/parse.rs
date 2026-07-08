// Index/note parsing: project-task extraction + newest-handoff resolution.

use std::{
    path::{Path, PathBuf},
    sync::LazyLock,
};

use regex::Regex;

static SECTION_HEADER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^##\s+(?P<name>.+?)\s*$").unwrap());
static LINK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^\s*-\s*(?:\[ \]\s*)?\[\[(?P<id>[A-Z]{2,4}-\d{4})(?:\|(?P<alias>[^\]]+))?\]\].*$",
    )
    .unwrap()
});
static INLINE_LEGACY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^(?P<indent>\s*)- \[ \] `(?P<session>[^`]+)`\s*(?:<-+|::)\s*(?P<prompt>.+?)\s*$",
    )
    .unwrap()
});
static FENCED_SESSION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?ms)^(?P<indent>\s*)- \[ \] `(?P<session>[^`]+)`\s*\r?\n```text\r?\n(?P<prompt>.*?)\r?\n```",
    )
    .unwrap()
});

use super::{
    errors::PendingWorkError,
    model::Item,
    naming::path_str,
    obsidian::store::ObsidianStore,
    text::{is_placeholder_prompt, line_number},
};

const ISSUE_NO_REPO: &str =
    "Project note is not mapped to a repo; add it to config/pending-work.json.";
const ISSUE_PLACEHOLDER_PROMPT: &str =
    "Prompt is a placeholder; define a real prompt before launching.";

/// Scan `<repo>/docs/handoffs/*.md`, exclude LEDGER.md/README.md (case-insensitive),
/// sort by (`LastWriteTime`, Name) DESC, take first.
pub fn newest_handoff(repo: &str) -> Result<PathBuf, String> {
    newest_handoff_typed(repo).map_err(String::from)
}

pub(super) fn newest_handoff_typed(repo: &str) -> Result<PathBuf, PendingWorkError> {
    let handoff_dir = Path::new(repo).join("docs").join("handoffs");
    if !handoff_dir.exists() {
        return Err(PendingWorkError::NoHandoffDirectory { path: handoff_dir });
    }
    let excluded = ["LEDGER.md", "README.md"];
    let mut entries: Vec<(std::time::SystemTime, String, PathBuf)> = Vec::new();
    for entry in std::fs::read_dir(&handoff_dir)
        .map_err(|source| PendingWorkError::ReadHandoffDirectory {
            path: handoff_dir.clone(),
            source,
        })?
        .flatten()
    {
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let name = p
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        if excluded.iter().any(|e| e.eq_ignore_ascii_case(&name)) {
            continue;
        }
        let mtime = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        entries.push((mtime, name, p));
    }
    if entries.is_empty() {
        return Err(PendingWorkError::NoHandoffMarkdown { path: handoff_dir });
    }
    // Sort by (LastWriteTime, Name) descending — Name is the deterministic tiebreak.
    entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    Ok(entries.into_iter().next().unwrap().2)
}

/// Normalized section governing byte `offset`, or `None` for the normal region.
fn section_at(text: &str, offset: usize) -> Option<String> {
    let mut current: Option<String> = None;
    for m in SECTION_HEADER_RE.captures_iter(text) {
        if m.get(0).unwrap().start() >= offset {
            break;
        }
        current = match m["name"].trim().to_lowercase().as_str() {
            "future" | "futuro" => Some("Future".to_string()),
            "human" => Some("Human".to_string()),
            "low-prio" | "low-priority" => Some("Low-prio".to_string()),
            _ => Some(m["name"].trim().to_string()),
        };
    }
    current
}

/// Parse file-model links + legacy checkbox items.
pub fn get_project_tasks(project: &str, repo: Option<&str>, index_path: &Path) -> Vec<Item> {
    let Some(text) = ObsidianStore::read_text_optional(index_path) else {
        return Vec::new();
    };
    parse_project_tasks_from_text(project, repo, index_path, &text, |item_path| {
        item_path
            .exists()
            .then(|| ObsidianStore::read_text_or_default(item_path))
    })
}

/// One legacy inline-checkbox match: byte span + parsed session/prompt.
struct LegacyMatch {
    start: usize,
    len: usize,
    session: String,
    prompt: String,
}

fn parse_project_tasks_from_text(
    project: &str,
    repo: Option<&str>,
    index_path: &Path,
    text: &str,
    load_item_note: impl Fn(&Path) -> Option<String>,
) -> Vec<Item> {
    let dir = index_path.parent().unwrap_or(Path::new("."));
    let note = path_str(index_path);
    let mut items = parse_file_model_items(project, repo, dir, &note, text, load_item_note);
    items.extend(parse_legacy_items(project, repo, &note, text));
    items
}

/// File-model items: `- [[ID|alias]]` / `- [[ID]]` wikilinks, one [`Item`] per
/// link, backed by an on-disk `{ID}.md` note read via `load_item_note`.
fn parse_file_model_items(
    project: &str,
    repo: Option<&str>,
    dir: &Path,
    note: &str,
    text: &str,
    load_item_note: impl Fn(&Path) -> Option<String>,
) -> Vec<Item> {
    let mut items: Vec<Item> = Vec::new();

    // ! Width [A-Z]{2,4} is intentional, though current prefixes are all 3 letters.
    // * Optional "- [ ] " prefix matches the Obsidian checkbox form; "- [x]" (done) is
    // * deliberately not matched, so ticking the box in Obsidian drops the item from open lists.
    for m in LINK_RE.captures_iter(text) {
        let id = m["id"].to_string();
        let alias = m
            .name("alias")
            .map(|a| a.as_str().to_string())
            .unwrap_or_default();
        let item_path = dir.join(format!("{id}.md"));
        let mut issues: Vec<String> = Vec::new();

        if repo.is_none_or(|r| r.trim().is_empty()) {
            issues.push(ISSUE_NO_REPO.to_string());
        }

        let mut title = alias.clone();
        let mut prompt = String::new();
        let mut prereq: Option<String> = None;
        let mut effort: Option<String> = None;
        let mut created: Option<String> = None;

        if let Some(raw) = load_item_note(&item_path) {
            let parsed = crate::frontmatter::parse(&raw);
            if let Some(t) = parsed.frontmatter.get("title")
                && !t.is_empty()
            {
                title.clone_from(t);
            }
            prereq = parsed
                .frontmatter
                .get("prereq")
                .filter(|v| !v.trim().is_empty())
                .cloned();
            effort = parsed
                .frontmatter
                .get("effort")
                .filter(|v| !v.trim().is_empty())
                .cloned();
            created = parsed
                .frontmatter
                .get("created")
                .filter(|v| !v.trim().is_empty())
                .cloned();
            prompt = parsed.body.trim().to_string();
        } else {
            issues.push(format!("Work-item note missing: {}", item_path.display()));
        }
        if title.trim().is_empty() {
            title.clone_from(&id);
        }
        if is_placeholder_prompt(&prompt) {
            issues.push(ISSUE_PLACEHOLDER_PROMPT.to_string());
        }

        let match_start = m.get(0).unwrap().start();
        let match_len = m.get(0).unwrap().len();
        let launchable = issues.is_empty();
        let needs_prompt = is_placeholder_prompt(&prompt);

        items.push(Item {
            id,
            project: project.to_string(),
            session: title,
            prompt,
            repo: repo.map(std::string::ToString::to_string),
            note: note.to_string(),
            file_path: Some(path_str(&item_path)),
            line: line_number(text, match_start),
            format: "file".to_string(),
            marker_index: match_start,
            marker_length: match_len,
            launchable,
            needs_prompt,
            issues,
            section: section_at(text, match_start),
            prereq,
            effort,
            created,
        });
    }
    items
}

/// Legacy safety net: inline backtick checkboxes.
/// The Rust regex crate does not support lookahead, so the fenced form is split:
/// first try the normal closing fence, then the checkpoint-at-next-checkbox form.
fn parse_legacy_items(project: &str, repo: Option<&str>, note: &str, text: &str) -> Vec<Item> {
    let mut legacy: Vec<LegacyMatch> = Vec::new();
    for m in INLINE_LEGACY_RE.captures_iter(text) {
        legacy.push(LegacyMatch {
            start: m.get(0).unwrap().start(),
            len: m.get(0).unwrap().len(),
            session: m["session"].to_string(),
            prompt: m["prompt"].trim().to_string(),
        });
    }
    for m in FENCED_SESSION_RE.captures_iter(text) {
        legacy.push(LegacyMatch {
            start: m.get(0).unwrap().start(),
            len: m.get(0).unwrap().len(),
            session: m["session"].to_string(),
            prompt: m["prompt"].trim().to_string(),
        });
    }
    legacy.sort_by_key(|l| l.start);

    let mut items: Vec<Item> = Vec::new();
    let mut ordinal = 0usize;
    for item in legacy {
        ordinal += 1;
        let mut issues: Vec<String> = Vec::new();
        if repo.is_none_or(|r| r.trim().is_empty()) {
            issues.push(ISSUE_NO_REPO.to_string());
        }
        if is_placeholder_prompt(&item.prompt) {
            issues.push(ISSUE_PLACEHOLDER_PROMPT.to_string());
        }
        let launchable = issues.is_empty();
        let needs_prompt = is_placeholder_prompt(&item.prompt);
        items.push(Item {
            id: format!("{project}:{ordinal}"),
            project: project.to_string(),
            session: item.session,
            prompt: item.prompt,
            repo: repo.map(std::string::ToString::to_string),
            note: note.to_string(),
            file_path: None,
            line: line_number(text, item.start),
            format: "legacy".to_string(),
            marker_index: item.start,
            marker_length: item.len,
            launchable,
            needs_prompt,
            issues,
            section: section_at(text, item.start),
            prereq: None,
            effort: None,
            created: None,
        });
    }

    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_project_tasks_from_text_uses_supplied_item_note_text() {
        let index_path = Path::new("notes/glep-shimeji/glep-shimeji.md");
        let index_text = "# glep-shimeji\n- [[GLP-0001|fallback title]]\n";
        let items = parse_project_tasks_from_text(
            "glep-shimeji",
            Some("/repo"),
            index_path,
            index_text,
            |item_path| {
                assert_eq!(item_path, Path::new("notes/glep-shimeji/GLP-0001.md"));
                Some(
                    "---\nstatus: active\ntitle: tray gui\nproject: glep-shimeji\ncreated: 2026-01-01\nprereq: \"[[GLP-0000]]\"\n---\n\nadd startup toggle\n"
                        .to_string(),
                )
            },
        );

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "GLP-0001");
        assert_eq!(items[0].session, "tray gui");
        assert_eq!(items[0].prompt, "add startup toggle");
        assert_eq!(items[0].prereq.as_deref(), Some("\"[[GLP-0000]]\""));
        assert!(items[0].launchable);
    }

    #[test]
    fn parse_project_tasks_from_text_reads_effort_frontmatter() {
        let index_path = Path::new("notes/glep-shimeji/glep-shimeji.md");
        let index_text = "# glep-shimeji\n- [[GLP-0001|tray gui]]\n";
        let items = parse_project_tasks_from_text(
            "glep-shimeji",
            Some("/repo"),
            index_path,
            index_text,
            |item_path| {
                assert_eq!(item_path, Path::new("notes/glep-shimeji/GLP-0001.md"));
                Some(
                    "---\nstatus: active\ntitle: tray gui\nproject: glep-shimeji\ncreated: 2026-01-01\neffort: 3\n---\n\nadd startup toggle\n"
                        .to_string(),
                )
            },
        );

        assert_eq!(items[0].effort.as_deref(), Some("3"));
    }

    #[test]
    fn parse_project_tasks_from_text_reads_created_frontmatter() {
        let index_path = Path::new("notes/glep-shimeji/glep-shimeji.md");
        let index_text = "# glep-shimeji\n- [[GLP-0001|tray gui]]\n";
        let items = parse_project_tasks_from_text(
            "glep-shimeji",
            Some("/repo"),
            index_path,
            index_text,
            |_| {
                Some(
                    "---\nstatus: active\ntitle: tray gui\nproject: glep-shimeji\ncreated: 2026-01-01\n---\n\nadd startup toggle\n"
                        .to_string(),
                )
            },
        );

        assert_eq!(items[0].created.as_deref(), Some("2026-01-01"));
    }

    #[test]
    fn parse_project_tasks_from_text_effort_absent_by_default() {
        let index_path = Path::new("notes/glep-shimeji/glep-shimeji.md");
        let index_text = "# glep-shimeji\n- [[GLP-0001|tray gui]]\n";
        let items = parse_project_tasks_from_text(
            "glep-shimeji",
            Some("/repo"),
            index_path,
            index_text,
            |_| {
                Some(
                    "---\nstatus: active\ntitle: tray gui\nproject: glep-shimeji\ncreated: 2026-01-01\n---\n\nadd startup toggle\n"
                        .to_string(),
                )
            },
        );

        assert_eq!(items[0].effort, None);
    }

    #[test]
    fn parse_project_tasks_from_text_preserves_low_prio_human_and_future_sections() {
        let index_path = Path::new("notes/glep-shimeji/glep-shimeji.md");
        let index_text = "- [ ] [[GLP-0001|normal]]\n\n## Low-prio\n- [ ] [[GLP-0002|low]]\n\n## Human\n- [ ] [[GLP-0003|human]]\n\n## Future\n- [ ] [[GLP-0004|future]]\n";
        let items = parse_project_tasks_from_text(
            "glep-shimeji",
            Some("/repo"),
            index_path,
            index_text,
            |_| {
                Some(
                    "---\nstatus: active\ntitle: title\nproject: glep-shimeji\ncreated: 2026-01-01\n---\n\nbody\n"
                        .to_string(),
                )
            },
        );

        assert_eq!(items.len(), 4);
        assert_eq!(items[0].section, None);
        assert_eq!(items[1].section.as_deref(), Some("Low-prio"));
        assert_eq!(items[2].section.as_deref(), Some("Human"));
        assert_eq!(items[3].section.as_deref(), Some("Future"));
    }
}
