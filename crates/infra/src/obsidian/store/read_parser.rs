use std::sync::LazyLock;

use regex::Regex;

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
        r"(?m)^(?P<indent>[ \t]*)- \[ \] `(?P<session>[^`]+)`\s*(?:<-+|::)\s*(?P<prompt>.+?)\s*$",
    )
    .expect("valid inline legacy regex")
});
static FENCED_SESSION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?ms)^(?P<indent>[ \t]*)- \[ \] `(?P<session>[^`]+)`\s*\r?\n```text\r?\n(?P<prompt>.*?)\r?\n```",
    )
    .expect("valid fenced legacy regex")
});

/// An open wikilink entry (`- [[ID]]` / `- [ ] [[ID|alias]]`) in an index.
pub(super) struct LinkEntry {
    pub(super) id: String,
    pub(super) alias: Option<String>,
    pub(super) start: usize,
}

/// A legacy inline prompt entry (backtick session + inline or fenced prompt).
pub(super) struct InlineEntry {
    pub(super) session: String,
    pub(super) prompt: String,
    pub(super) start: usize,
}

/// Every open item entry in an index: wikilinks in document order, inline
/// legacy prompts sorted by position (their 1-based rank is the inline
/// ordinal). The single scan the generic record materialization
/// (`item_record::list_pending_items`) consumes.
pub(super) struct IndexScan {
    pub(super) links: Vec<LinkEntry>,
    pub(super) inline: Vec<InlineEntry>,
}

pub(super) fn scan_index(text: &str) -> IndexScan {
    let links = LINK_RE
        .captures_iter(text)
        .map(|captures| LinkEntry {
            id: captures["id"].to_string(),
            alias: captures
                .name("alias")
                .map(|alias| alias.as_str().to_string()),
            start: captures.get(0).expect("whole match").start(),
        })
        .collect();

    let mut inline: Vec<InlineEntry> = INLINE_LEGACY_RE
        .captures_iter(text)
        .chain(FENCED_SESSION_RE.captures_iter(text))
        .map(|captures| InlineEntry {
            session: captures["session"].to_string(),
            prompt: captures["prompt"].trim().to_string(),
            start: captures.get(0).expect("whole match").start(),
        })
        .collect();
    inline.sort_by_key(|entry| entry.start);

    IndexScan { links, inline }
}

/// The RAW trimmed `## <label>` header governing `offset`, if any.
/// Normalization (`Futuro` → `Future`, …) is application policy
/// ([`enrich::normalize_section_label`]).
pub(super) fn section_label_at(text: &str, offset: usize) -> Option<String> {
    let mut current = None;
    for captures in SECTION_HEADER_RE.captures_iter(text) {
        if captures.get(0).expect("whole match").start() >= offset {
            break;
        }
        current = Some(captures["name"].trim().to_string());
    }
    current
}

pub(super) fn line_number(text: &str, index: usize) -> usize {
    if index == 0 {
        return 1;
    }
    1 + text[..index].matches('\n').count()
}
