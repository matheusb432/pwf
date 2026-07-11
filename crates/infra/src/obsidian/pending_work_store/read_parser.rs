use std::{path::Path, sync::LazyLock};

use pwf_domain::pending_work::OpenItem;
use regex::Regex;

use super::fs::path_str;

static SECTION_HEADER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^##\s+(?P<name>.+?)\s*$").expect("valid section regex"));
static LINK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^\s*-\s*(?:\[ \]\s*)?\[\[(?P<id>[A-Z]{2,4}-\d{4})(?:\|(?P<alias>[^\]]+))?\]\].*$",
    )
    .expect("valid item link regex")
});
static INLINE_LEGACY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^(?P<indent>\s*)- \[ \] `(?P<session>[^`]+)`\s*(?:<-+|::)\s*(?P<prompt>.+?)\s*$",
    )
    .expect("valid inline legacy regex")
});
static FENCED_SESSION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?ms)^(?P<indent>\s*)- \[ \] `(?P<session>[^`]+)`\s*\r?\n```text\r?\n(?P<prompt>.*?)\r?\n```",
    )
    .expect("valid fenced legacy regex")
});
static PLACEHOLDER_PROMPT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(^\s*\[!\]\s*TODO\b|^\s*TODO\b|definir prompt|define prompt|tbd)")
        .expect("valid placeholder regex")
});

const ISSUE_NO_REPO: &str =
    "Project note is not mapped to a repo; add it to config/pending-work.json.";
const ISSUE_PLACEHOLDER_PROMPT: &str =
    "Prompt is a placeholder; define a real prompt before launching.";

pub(super) fn parse_project_tasks(
    project: &str,
    repo: Option<&str>,
    index_path: &Path,
    text: &str,
    load_item_note: impl Fn(&Path) -> Option<String>,
) -> Vec<OpenItem> {
    let dir = index_path.parent().unwrap_or(Path::new("."));
    let note = path_str(index_path);
    let mut items = parse_file_model_items(project, repo, dir, &note, text, &load_item_note);
    items.extend(parse_legacy_items(project, repo, &note, text));
    items
}

fn parse_file_model_items(
    project: &str,
    repo: Option<&str>,
    dir: &Path,
    note: &str,
    text: &str,
    load_item_note: &impl Fn(&Path) -> Option<String>,
) -> Vec<OpenItem> {
    let mut items = Vec::new();

    for captures in LINK_RE.captures_iter(text) {
        let id = captures["id"].to_string();
        let alias = captures
            .name("alias")
            .map(|alias| alias.as_str().to_string())
            .unwrap_or_default();
        let item_path = dir.join(format!("{id}.md"));
        let mut issues = Vec::new();

        if repo.is_none_or(|value| value.trim().is_empty()) {
            issues.push(ISSUE_NO_REPO.to_string());
        }

        let mut title = alias;
        let mut prompt = String::new();
        let mut prereq = None;
        let mut effort = None;
        let mut tags = None;
        let mut created = None;

        if let Some(raw) = load_item_note(&item_path) {
            let parsed = pwf_core::frontmatter::parse(&raw);
            if let Some(frontmatter_title) = parsed.frontmatter.get("title")
                && !frontmatter_title.is_empty()
            {
                title.clone_from(frontmatter_title);
            }
            prereq = parsed
                .frontmatter
                .get("prereq")
                .filter(|value| !value.trim().is_empty())
                .cloned();
            effort = parsed
                .frontmatter
                .get("effort")
                .filter(|value| !value.trim().is_empty())
                .cloned();
            tags = parsed.frontmatter.get("tags").cloned();
            created = parsed
                .frontmatter
                .get("created")
                .filter(|value| !value.trim().is_empty())
                .cloned();
            prompt = parsed.body.trim().to_string();
        } else {
            issues.push(format!("Work-item note missing: {}", item_path.display()));
        }

        if title.trim().is_empty() {
            title.clone_from(&id);
        }
        let needs_prompt = is_placeholder_prompt(&prompt);
        if needs_prompt {
            issues.push(ISSUE_PLACEHOLDER_PROMPT.to_string());
        }

        let matched = captures.get(0).expect("whole match");
        items.push(OpenItem {
            id,
            project: project.to_string(),
            session: title,
            prompt,
            repo: repo.map(str::to_string),
            note: note.to_string(),
            item_file: Some(path_str(&item_path)),
            line: line_number(text, matched.start()),
            format: "file".to_string(),
            launchable: issues.is_empty(),
            needs_prompt,
            issues,
            section: section_at(text, matched.start()),
            prereq,
            effort,
            tags,
            created,
        });
    }

    items
}

fn parse_legacy_items(project: &str, repo: Option<&str>, note: &str, text: &str) -> Vec<OpenItem> {
    let mut legacy_matches = Vec::new();
    for captures in INLINE_LEGACY_RE.captures_iter(text) {
        legacy_matches.push(LegacyMatch {
            start: captures.get(0).expect("whole match").start(),
            session: captures["session"].to_string(),
            prompt: captures["prompt"].trim().to_string(),
        });
    }
    for captures in FENCED_SESSION_RE.captures_iter(text) {
        legacy_matches.push(LegacyMatch {
            start: captures.get(0).expect("whole match").start(),
            session: captures["session"].to_string(),
            prompt: captures["prompt"].trim().to_string(),
        });
    }
    legacy_matches.sort_by_key(|item| item.start);

    legacy_matches
        .into_iter()
        .enumerate()
        .map(|(index, item)| {
            let mut issues = Vec::new();
            if repo.is_none_or(|value| value.trim().is_empty()) {
                issues.push(ISSUE_NO_REPO.to_string());
            }
            let needs_prompt = is_placeholder_prompt(&item.prompt);
            if needs_prompt {
                issues.push(ISSUE_PLACEHOLDER_PROMPT.to_string());
            }

            OpenItem {
                id: format!("{project}:{}", index + 1),
                project: project.to_string(),
                session: item.session,
                prompt: item.prompt,
                repo: repo.map(str::to_string),
                note: note.to_string(),
                item_file: None,
                line: line_number(text, item.start),
                format: "legacy".to_string(),
                launchable: issues.is_empty(),
                needs_prompt,
                issues,
                section: section_at(text, item.start),
                prereq: None,
                effort: None,
                tags: None,
                created: None,
            }
        })
        .collect()
}

struct LegacyMatch {
    start: usize,
    session: String,
    prompt: String,
}

fn section_at(text: &str, offset: usize) -> Option<String> {
    let mut current = None;
    for captures in SECTION_HEADER_RE.captures_iter(text) {
        if captures.get(0).expect("whole match").start() >= offset {
            break;
        }
        current = match captures["name"].trim().to_lowercase().as_str() {
            "future" | "futuro" => Some("Future".to_string()),
            "human" => Some("Human".to_string()),
            "low-prio" | "low-priority" => Some("Low-prio".to_string()),
            _ => Some(captures["name"].trim().to_string()),
        };
    }
    current
}

fn is_placeholder_prompt(prompt: &str) -> bool {
    prompt.trim().is_empty() || PLACEHOLDER_PROMPT_RE.is_match(prompt)
}

fn line_number(text: &str, index: usize) -> usize {
    if index == 0 {
        return 1;
    }
    1 + text[..index].matches('\n').count()
}
