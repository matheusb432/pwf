use std::sync::LazyLock;

use regex::Regex;

static SECTION_HEADER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^##\s+(?P<name>.+?)\s*$").expect("valid section regex"));
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

/// Contains an inline prompt and its source offset.
pub(super) struct InlineEntry {
    pub(super) session: String,
    pub(super) prompt: String,
    pub(super) start: usize,
}

/// Scans inline prompts in document order.
///
/// An inline prompt's one-based rank becomes its ordinal identity.
pub(super) struct IndexScan {
    pub(super) inline: Vec<InlineEntry>,
}

pub(super) fn scan_index(text: &str) -> IndexScan {
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

    IndexScan { inline }
}

/// Returns the raw trimmed H2 label governing `offset`, if any.
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
